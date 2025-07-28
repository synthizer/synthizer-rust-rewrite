use std::time::Duration;

use crate::config::{BLOCK_SIZE, MAX_CHANNELS};
use crate::core_traits::*;
use crate::sample_sources::Descriptor;
use crate::unique_id::UniqueId;
use crate::Chain;
use crate::ChannelFormat;

/// A reference to some media (AKA files, etc) in a mount.
///
/// Media is like slots except that the batch lets you do specific media operations rather than flat out replacements.
/// Media changes (but not creation/initial signal usage) flush the batch to the audio device, so that the I/O may be
/// performed in a context where you can get a `Result` back.
///
/// If you want to use fully in-memory, decoded assets, consider wavetables instead.  This can be used for those but is
/// slower and less capable.
///
/// The methods directly on this struct allow one to get information on the media underneath, e.g. the channel format
/// and duration.
///
/// Like slots, media references only become valid after they are integrated into a mount.  That said, this type may
/// only be converted into a signal once. Further attempts to convert this into a signal will fail, as doing so requires
/// consuming some inner state.
///
/// Operations to media happen in a background thread, with the exception of play/pause which are intentionally
/// integrated into the rest of the batch.  You must add your own fade-in and fade-out mechanisms.  For seeking and loop
/// configuration, we have no choice but to offload to a background thread because these are I/O operations, which is a
/// long way to say that you will not be able to properly account for those.  This is the biggest reason to use a
/// wavetable.  For the sake of fade-in and fade-out you can use a slot: the changes to the slot will apply with the
/// play/pause properly.
pub struct Media {
    pub(crate) descriptor: Descriptor,

    pub(crate) ring: Option<audio_synchronization::spsc_ring::RingReader<f32>>,
}

impl Media {
    pub fn get_channel_format(&self) -> ChannelFormat {
        self.descriptor.channel_format
    }

    /// Get the duration.
    pub fn get_duration(&self) -> Duration {
        Duration::from_secs_f64(self.descriptor.duration as f64 / crate::config::SR as f64)
    }

    /// Convert this media to a signal.
    ///
    /// This infallible method may only be called once. Duplicate calls panic.
    ///
    /// You must pick the output format e.g. stereo, mono, etc. You must also pick the maximum number of channels, which
    /// tunes the size of the frames on the stack.  The resulting signal outputs an array `[f64; MAX_CHANS]`, where any
    /// extra channels are zeroed, and missing channels discarded
    pub fn start_chain<const MAX_CHANS: usize>(
        &mut self,
        wanted_format: ChannelFormat,
    ) -> Chain<impl IntoSignal<Signal = impl Signal<Input = (), Output = [f64; MAX_CHANS]>>> {
        Chain {
            inner: MediaSignalConfig::<MAX_CHANS> {
                descriptor: self.descriptor.clone(),
                wanted_format,
                ring: self.ring.take().expect("Can only call once"),
            },
        }
    }
}

struct MediaSignalState {
    ring: audio_synchronization::spsc_ring::RingReader<f32>,

    /// Filled at the beginning of each block. Then drained out on ticks.  `actual_chans * BLOCK_SIZE` in size.
    buffer: Vec<f64>,

    /// Advanced over the block, then reset.
    buffer_consumed: usize,

    descriptor: Descriptor,

    wanted_format: ChannelFormat,
}

struct MediaSignalConfig<const MAX_CHANS: usize> {
    descriptor: Descriptor,
    wanted_format: ChannelFormat,
    ring: audio_synchronization::spsc_ring::RingReader<f32>,
}

struct MediaSignal<const CHANS: usize>(());

unsafe impl<const MAX_CHANS: usize> Signal for MediaSignal<MAX_CHANS> {
    type Input = ();
    type Output = [f64; MAX_CHANS];
    type State = MediaSignalState;

    fn on_block_start(
        _ctx: &crate::context::SignalExecutionContext<'_, '_>,
        state: &mut Self::State,
    ) {
        let mut rbuf = [0.0f32; MAX_CHANNELS * BLOCK_SIZE];

        state.buffer.fill(0.0f64);
        state.buffer_consumed = 0;

        state
            .ring
            .read_to_slice(&mut rbuf[..BLOCK_SIZE * state.descriptor.get_channel_count()]);

        let rbuf_f64: [[f64; MAX_CHANS]; BLOCK_SIZE] = std::array::from_fn(|i| {
            let frame = i * state.descriptor.get_channel_count();
            std::array::from_fn(|ch| rbuf[frame + ch] as f64)
        });
        let mut rbuf_converted: [[f64; MAX_CHANS]; BLOCK_SIZE] = [[0.0; MAX_CHANS]; BLOCK_SIZE];

        crate::channel_conversion::convert_channels(
            &rbuf_f64,
            state.descriptor.channel_format,
            &mut rbuf_converted,
            state.wanted_format,
        );

        rbuf_converted.iter().enumerate().for_each(|(frame, val)| {
            let chs = state.wanted_format.get_channel_count().get();
            let off = frame * state.wanted_format.get_channel_count().get();
            state.buffer[off..(off + chs)].copy_from_slice(val);
        });
    }

    fn tick_frame(
        _ctx: &'_ crate::context::SignalExecutionContext<'_, '_>,
        _input: Self::Input,
        state: &mut Self::State,
    ) -> Self::Output {
        let mut output_frame = [0.0f64; MAX_CHANS];

        let chan_count = state.wanted_format.get_channel_count().get();
        let frame_off = state.buffer_consumed * chan_count;

        output_frame
            .iter_mut()
            .take(chan_count)
            .enumerate()
            .for_each(|(i, dest)| {
                *dest = state.buffer[frame_off + i];
            });

        state.buffer_consumed += 1;

        output_frame
    }
}

impl<const MAX_CHANS: usize> IntoSignal for MediaSignalConfig<MAX_CHANS> {
    type Signal = MediaSignal<MAX_CHANS>;

    fn into_signal(self) -> IntoSignalResult<Self> {
        let descriptor = self.descriptor;
        let media_chans = descriptor.channel_format.get_channel_count().get();
        Ok(ReadySignal {
            signal: MediaSignal::<MAX_CHANS>(()),
            state: MediaSignalState {
                buffer: vec![0.0f64; media_chans * BLOCK_SIZE],
                buffer_consumed: 0,
                descriptor,
                wanted_format: self.wanted_format,
                ring: self.ring,
            },
        })
    }

    fn trace<F: FnMut(UniqueId, TracedResource)>(
        &mut self,
        _inserter: &mut F,
    ) -> crate::Result<()> {
        Ok(())
    }
}
