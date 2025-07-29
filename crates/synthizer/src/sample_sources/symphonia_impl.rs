use std::num::{NonZeroU64, NonZeroUsize};

use symphonia::core::{
    audio::{AudioBuffer, Signal},
    codecs::CodecParameters,
    codecs::Decoder,
    errors::Result as SResult,
    formats::FormatReader,
    io::{MediaSource, MediaSourceStream},
};

use crate::config::*;
use crate::error::Result;
use crate::sample_sources::Descriptor;

/// Internal wrapper which wraps Symphonia for media decoding.
pub(crate) struct SymphoniaWrapper {
    format: Box<dyn FormatReader + 'static>,
    decoder: Box<dyn Decoder>,

    descriptor: Descriptor,

    track_index: usize,

    /// This internal buffer of samples fills up and potentially grows as data is read from Symphonia, which cannot
    /// tell us the size of the next packet because media formats don't know that information.
    ///
    /// This is always some number of complete frames.
    buffer: AudioBuffer<f32>,

    /// We cannot use our SplittableWrapper because the size of the underlying buffer will frequently change, so we must maintain the counter ourselves.
    ///
    /// This is in frames, thus the name.
    buffer_read_frames: usize,

    is_at_eof: bool,
}

fn codec_params_to_channel_format(
    params: &CodecParameters,
) -> Option<crate::channel_format::ChannelFormat> {
    use crate::channel_format::ChannelFormat as CF;

    // If we have a format from Symphonia which can map to a format from Synthizer, just do that.
    if let Some(f) = params.channel_layout {
        use symphonia::core::audio::Layout as L;

        let format = match f {
            L::Mono => CF::Mono,
            L::Stereo => CF::Stereo,
            L::FivePointOne => CF::Raw {
                channels: NonZeroUsize::new(6).unwrap(),
            },
            L::TwoPointOne => CF::Raw {
                channels: NonZeroUsize::new(3).unwrap(),
            },
        };
        Some(format)
    } else if let Some(mask) = params.channels {
        // We can otherwise try to guess from the length of the channel mask.
        let channel_count = mask.bits().count_ones();

        match channel_count {
            0 => None,
            1 => Some(CF::Mono),
            2 => Some(CF::Stereo),
            x if x < MAX_CHANNELS as u32 => Some(CF::Raw {
                channels: NonZeroUsize::new(x as usize).unwrap(),
            }),
            _ => None,
        }
    } else {
        None
    }
}

pub(crate) fn build_symphonia_maybe_nodur<S: MediaSource + 'static>(
    source: S,
) -> SResult<(SymphoniaWrapper, bool)> {
    let probe = symphonia::default::get_probe();
    let source_stream = MediaSourceStream::new(Box::new(source), Default::default());

    let format = probe.format(
        &Default::default(),
        source_stream,
        &Default::default(),
        &Default::default(),
    )?;
    let format = format.format;

    // We always decode the first decodable track, and cannot if there aren't any.
    let track_index = format
        .tracks()
        .iter()
        .position(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                "This source's format was recognized, but has no decodable audio tracks",
            )
        })?;

    // Try to get a Synthizer-type format.  If we can't, this is also a stream we cannot handle.
    let synthizer_channel_format = codec_params_to_channel_format(
        &format.tracks()[track_index].codec_params,
    )
    .ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            "The first audio track does not contain a channel format compatible with Synthizer",
        )
    })?;

    // To simplify other code, use 0 for the samplerate so that we can create the initial instance, then fill the
    // internal buffer and pull it from there if needed.
    let (sample_rate, needs_first_block) = format.tracks()[track_index]
        .codec_params
        .sample_rate
        .map(|x| (x as u64, false))
        .unwrap_or((0, true));

    let codec_registry = symphonia::default::get_codecs();
    let decoder = codec_registry.make(
        &format.tracks()[track_index].codec_params,
        &Default::default(),
    )?;

    let duration_from_meta = format.tracks()[track_index].codec_params.n_frames;

    let descriptor = Descriptor {
        duration: duration_from_meta.unwrap_or(0),
        channel_format: synthizer_channel_format,
        sample_rate: NonZeroU64::new(sample_rate).unwrap(),
    };

    let mut ret = SymphoniaWrapper {
        decoder,
        format,
        descriptor,
        track_index,
        buffer: AudioBuffer::unused(),
        buffer_read_frames: 0,
        is_at_eof: false,
    };

    if needs_first_block {
        if !ret.refill_buffer()? {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "This source returned EOF on the first packet",
            )
            .into());
        }

        let possible_sr = ret.buffer.spec().rate as u64;
        if possible_sr == 0 {
            // Really, really, really shouldn't be possible.
            return Err(std::io::Error::new(std::io::ErrorKind::Other, "This source's first packet of data has a samplerate of 0, which implies that it is corrupt").into());
        }

        ret.descriptor.sample_rate = NonZeroU64::new(possible_sr).unwrap();
    }

    Ok((ret, duration_from_meta.is_some()))
}

pub(crate) fn build_symphonia<S: MediaSource + 'static>(source: S) -> Result<SymphoniaWrapper> {
    use std::io::Seek;

    let (mut ret, durgood) = build_symphonia_maybe_nodur(source)?;

    if !durgood {
        let mut frames_so_far = 0u64;
        loop {
            frames_so_far += ret.buffer.frames() as u64;
            ret.buffer.clear();
            if !ret.refill_buffer()? {
                break;
            }
        }

        let mut inner = ret.format.into_inner();
        inner.rewind()?;
        let mut good = build_symphonia_maybe_nodur(inner)?.0;
        good.descriptor.duration = frames_so_far;
        return Ok(good);
    }
    Ok(ret)
}

/// Check if this error is an end-of-stream.
///
/// Symphonia's example at https://github.com/pdeljanov/Symphonia/blob/master/symphonia-play/src/main.rs says that the
/// check is UnexpectedEof, then a string comparison.  We drop the string comparison, because we intentionally limit
/// what we will accept, and so the only thing that could ever give us UnexpectedEof is Symphonia, not the I/O source.
///
/// Weak end-of-stream handling makes sense, kind of.  But it's not really great for our use case.  This alone may be a
/// sufficient reason to migrate off Symphonia, but that's TBD.  I've opened an issue for this:
/// https://github.com/pdeljanov/Symphonia/issues/246
fn err_is_eof(err: &symphonia::core::errors::Error) -> bool {
    use symphonia::core::errors::Error as E;

    matches!(err,
        E::IoError(i) if i.kind() == std::io::ErrorKind::UnexpectedEof)
}

impl SymphoniaWrapper {
    /// Fill the internal buffer, and reset the counter for read frames to 0.
    ///
    /// Returns `Ok(false)` to indicate EOF.
    ///
    /// also sets `is_at_eof` if needed.
    fn refill_buffer(&mut self) -> SResult<bool> {
        loop {
            // Note that the EOF case here only happens if we have not yet handled a packet, since this function handles
            // one packet at a time.
            let packet = match self.format.next_packet() {
                Ok(x) => x,
                Err(e) if err_is_eof(&e) => {
                    self.is_at_eof = true;
                    return Ok(false);
                }
                Err(e) => return Err(e),
            };

            let track_id = self.format.tracks()[self.track_index].id;
            if packet.track_id() != track_id {
                continue;
            }

            let abuf = self.decoder.decode(&packet)?;
            self.buffer = abuf.make_equivalent();
            abuf.convert(&mut self.buffer);
            self.buffer_read_frames = 0;
            return Ok(true);
        }
    }

    // Handle the complex seeking logic Symphonia wants.
    fn do_seeking(&mut self, sample: u64) -> SResult<()> {
        // Our buffer is invalid, whatever happens.
        self.buffer = AudioBuffer::unused();
        self.buffer_read_frames = 0;

        // The first step of seeking is to move the format to the course position.  Symphonia doesn't let us seek
        // samples even on formats which are sample-accurate, so we claim imprecise seeking, work out a timestamp, and
        // do our best.
        let ts_float = sample as f64 / self.descriptor.sample_rate.get() as f64;
        let ts = symphonia::core::units::Time {
            seconds: ts_float as u64,
            frac: ts_float - ts_float.floor(),
        };

        let seek_to = symphonia::core::formats::SeekTo::Time {
            time: ts,
            track_id: Some(self.format.tracks()[self.track_index].id),
        };

        let seek_res = self
            .format
            .seek(symphonia::core::formats::SeekMode::Accurate, seek_to)?;
        // if nothing else, the decoder must now reset.
        self.decoder.reset();
        // And we aren't eof.
        self.is_at_eof = false;

        // We need some number of samples to get from the result of our seek to the actual position.  To work this out,
        // figure out the time relative to the timebase and get that as f64 seconds, then it's a simple subtraction.
        //
        // If there is no timebase, stop early because we have done our best.
        let Some(time_base) = self.format.tracks()[self.track_index]
            .codec_params
            .time_base
        else {
            return Ok(());
        };

        let got_time = time_base.calc_time(seek_res.actual_ts);
        let got_time_f64 = got_time.seconds as f64 + got_time.frac;
        let delta = ts_float - got_time_f64;
        if delta <= 0.0 {
            return Ok(());
        }

        let mut samples_needed = (delta * self.descriptor.sample_rate.get() as f64) as u64;
        while samples_needed > 0 {
            if !self.refill_buffer()? {
                // EOF is our best.
                return Ok(());
            }

            let frames_avail = self.buffer.frames() as u64;
            // First, maybe we have a partial buffer.  This is 0 if not, which is fine.
            self.buffer_read_frames = frames_avail.saturating_sub(samples_needed) as usize;
            samples_needed = samples_needed.saturating_sub(frames_avail);
        }

        Ok(())
    }

    pub(crate) fn get_descriptor(&self) -> &Descriptor {
        &self.descriptor
    }

    pub(crate) fn read_samples(&mut self, destination: &mut [f32]) -> Result<u64> {
        let chan_count = self.descriptor.channel_format.get_channel_count().get();
        assert_eq!(destination.len() % chan_count, 0);
        let total_frames = destination.len() / chan_count;
        let mut next_frame = 0;

        if self.is_at_eof {
            return Ok(0);
        }

        while next_frame < total_frames {
            let avail = self.buffer.frames() - self.buffer_read_frames;
            let can_do = avail.min(total_frames - next_frame);

            let dest_this_time = &mut destination[next_frame * chan_count..];

            for ch in 0..chan_count {
                for f in 0..can_do {
                    dest_this_time[f * chan_count + ch] =
                        self.buffer.chan(ch)[self.buffer_read_frames + f];
                }
            }

            self.buffer_read_frames += can_do;
            next_frame += can_do;

            if self.buffer_read_frames == self.buffer.frames() && !self.refill_buffer()? {
                break;
            }
        }

        Ok(next_frame as u64)
    }

    pub(crate) fn seek(&mut self, position_in_frames: u64) -> Result<()> {
        Ok(self.do_seeking(position_in_frames)?)
    }
}
