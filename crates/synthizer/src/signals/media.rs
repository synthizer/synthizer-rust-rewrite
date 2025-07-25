use std::sync::Arc;
use std::time::Duration;

use rpds::HashTrieMapSync as Map;

use crate::config;
use crate::core_traits::*;
use crate::sample_sources::execution::Executor;
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
    pub(crate) media_id: UniqueId,
    pub(crate) descriptor: Descriptor,
    pub(crate) executor: Option<Arc<Executor>>,
}

impl Media {
    pub fn get_channel_format(&self) -> ChannelFormat {
        self.descriptor.channel_format
    }

    /// Get the duration, if known.
    ///
    /// Not all media sources know their duration. For example, streaming sources don't, nor do many formats which don't
    /// have a header.  This value is also not guaranteed to be precisely accurate, as lossy formats in particular don't
    /// necessarily report it accurately themselves.
    pub fn get_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f64(
            self.descriptor.duration? as f64 / config::SR as f64,
        ))
    }

    /// Convert this media to a signal.
    ///
    /// This infallible method may only be called once. Duplicate calls panic.
    ///
    /// You must pick the output format e.g. stereo, mono, etc. The media will be converted beforehand.  You must also
    /// pick the maximum number of channels, which tunes the size of the frames on the stack.  The resulting signal
    /// outputs an array `[f64; MAX_CHANS]`, where any extra channels are zeroed, and missing channels discarded
    pub fn start_chain<const MAX_CHANS: usize>(
        &mut self,
        wanted_format: ChannelFormat,
    ) -> Chain<impl IntoSignal<Signal = impl Signal<Input = (), Output = [f64; MAX_CHANS]>>> {
        Chain {
            inner: MediaSignalConfig::<MAX_CHANS> {
                wanted_format,
                executor: self.executor.take().expect("This can only be called once"),
                media_id: self.media_id,
            },
        }
    }
}

#[derive(Clone)]
pub(crate) struct MediaEntry {
    pub(crate) executor: Arc<Executor>,

    /// pausing isn't in the executor. It's in whether or not the signal decides to drain data.
    pub(crate) playing: bool,
}

impl MediaEntry {
    pub(crate) fn new(executor: Arc<Executor>) -> Self {
        Self {
            executor,
            playing: true,
        }
    }
}

/// A map of media sources, stored in a mount and received through the context.
pub(crate) type MediaExecutorMap = Map<UniqueId, MediaEntry>;

struct MediaSignalState {
    media_id: UniqueId,

    /// Filled at the beginning of each block. Then drained out on ticks.  `actual_chans * BLOCK_SIZE` in size.  Used to
    /// convert from f32 to f64.
    buffer: Vec<f32>,

    /// Advanced over the block, then reset.
    buffer_consumed: usize,

    descriptor: Descriptor,

    wanted_format: ChannelFormat,
}

struct MediaSignalConfig<const MAX_CHANS: usize> {
    wanted_format: ChannelFormat,
    executor: Arc<Executor>,
    media_id: UniqueId,
}

struct MediaSignal<const CHANS: usize>(());

unsafe impl<const MAX_CHANS: usize> Signal for MediaSignal<MAX_CHANS> {
    type Input = ();
    type Output = [f64; MAX_CHANS];
    type State = MediaSignalState;

    fn on_block_start(
        ctx: &crate::context::SignalExecutionContext<'_, '_>,
        state: &mut Self::State,
    ) {
        let media = ctx
            .fixed
            .media
            .get(&state.media_id)
            .expect("This should have been traced");

        state.buffer.fill(0.0f32);
        state.buffer_consumed = 0;

        if media.playing {
            if let Err(e) = media.executor.read_block(&mut state.buffer) {
                rt_error!("Media source stopped forever! {e}");
            }
        }
    }

    fn tick_frame(
        _ctx: &'_ crate::context::SignalExecutionContext<'_, '_>,
        _input: Self::Input,
        state: &mut Self::State,
    ) -> Self::Output {
        let mut intermediate_frame = [0.0f64; MAX_CHANS];
        let output_frame = [0.0f64; MAX_CHANS];

        let chan_count = state.descriptor.channel_format.get_channel_count().get();
        let frame_off = state.buffer_consumed * chan_count;

        // Copy from f32 buffer to f64 frame
        for c in 0..chan_count.min(MAX_CHANS) {
            intermediate_frame[c] = state.buffer[frame_off + c] as f64;
        }

        state.buffer_consumed += 1;

        // Convert channels for single frame
        let mut output_array = [output_frame];
        crate::channel_conversion::convert_channels(
            &[intermediate_frame],
            state.descriptor.channel_format,
            &mut output_array,
            state.wanted_format,
        );

        output_array[0]
    }
}

impl<const MAX_CHANS: usize> IntoSignal for MediaSignalConfig<MAX_CHANS> {
    type Signal = MediaSignal<MAX_CHANS>;

    fn into_signal(self) -> IntoSignalResult<Self> {
        let media_chans = self
            .executor
            .descriptor()
            .channel_format
            .get_channel_count()
            .get();
        Ok(ReadySignal {
            signal: MediaSignal::<MAX_CHANS>(()),
            state: MediaSignalState {
                buffer: vec![0.0f32; media_chans * config::BLOCK_SIZE],
                buffer_consumed: 0,
                media_id: self.media_id,
                descriptor: self.executor.descriptor().clone(),
                wanted_format: self.wanted_format,
            },
        })
    }

    fn trace<F: FnMut(UniqueId, TracedResource)>(&mut self, inserter: &mut F) -> crate::Result<()> {
        inserter(self.media_id, TracedResource::Media(self.executor.clone()));
        Ok(())
    }
}
