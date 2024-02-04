mod encoded;
pub(crate) mod execution;
mod vec_source;

pub use encoded::create_encoded_source;
pub use vec_source::*;

use std::num::NonZeroU64;

/// Kinds of seeking a source of samples may support.
///
/// All internal Synthizer sources are Imprecise or better unless otherwise noted; `None` and `ToBeginning` only
/// currently arise for external sources of I/O, e.g. other libraries or asking Synthizer to wrap a [std::io::Read]
/// impl.
#[derive(Debug, Copy, Clone, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum SeekSupport {
    /// This source supports no seeking whatsoever.
    ///
    /// Primarily, this happens when a user requests a source be created from [std::io::Read].
    None,

    /// This source may seek to the beginning, and only to the beginning.
    ///
    /// This is the minimum required to enable looping.  Seeks to sample 0 and only seeks to sample 0 are valid.  Seeks
    /// to sample 0 must be precise.
    ToBeginning,

    /// This source supports seeking, but it is not sample-accurate.
    ///
    /// This happens primarily with lossy audio formats which cannot seek directly to specific samples, and is generally
    /// rare in practice.  Most lossy formats are lossy only in the frequency domain, and do allow seeks to a specific
    /// timestamp.
    ///
    /// Such sources can loop, but cannot loop reliably.  So, for example, trying to construct a musical instrument
    /// probably won't work.
    ///
    /// Seeking imprecisely must uphold two requirements:
    ///
    /// - Seeks to sample 0 are accurate.
    /// - Seeks always land at a sample <= the requested value, mnever later.
    ///
    /// Violating either of these will break looping support, so sources that cannot guarantee it should likely not
    /// claim to support seeking.
    ///
    /// If the source's duration in samples is also provided, then seeks will never be performed past the end.  If this
    /// isn't provided, seeks past the end may occur.  In this case, erroring is appropriate but at the cost that the
    /// user will end up with a silent source thereafter.  An attempt should be made to seek to the end in such cases if
    /// possible, because seeking has to happen on an audio thread and cannot be validated early.
    Imprecise,

    /// This source can seek to a precise sample.
    ///
    /// This is the same as `Imprecise`, except that it is assumed that seeks are always accurate.  All other comments
    /// about `Imprecise` apply, including seeks past the end if the total length of the source is not known.
    SampleAccurate,
}

/// Latencies a source might operate at.
///
/// This determines how "fast" reading samples from the source is expected to be on average, and is divided into classes
/// based on resources it might use.  See the [SampleSource] trait for more information on how reading occurs.
///
/// Specifying a raw value in seconds is not supported because the in-practice latency varies per machine.  Synthizer
/// will schedule source execution on various background threads as appropriate, prioritizing sources by their latency and how
/// soon more data from them will be needed.
#[derive(Debug, Copy, Clone, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum Latency {
    /// This source is audio-thread-safe.  This means:
    ///
    /// - It does not allocate.
    /// - It does not interact with the kernel in any way.
    /// - Reading samples from it will always return in a bounded, short amount of time.
    /// - Decoding the source is lightweight enough that it is always faster than realtime.
    ///
    /// Synthizer does not guarantee that such sources will be run on the audio thread, but it will do so when possible.
    /// If this source takes too long, audio will glitch globally.  Even if a source is audio-thread-safe, consider
    /// whether it is fast enough to run on the audio thread before marking it as such.  Note that such sources will be
    /// asked for large amounts of data infrequently, so "is as fast as realtime" is not a sufficient criteria.
    ///
    /// A good example of an audio threadsafe source would be a vector of floats (available internally), something
    /// evaluating a mathematical function such as sin, or something decrypting an already-decoded audio stream with a
    /// small overhead per sample (for example ChaCha or hardware-accelerated RSA).
    ///
    /// be careful: most decoding libraries are not realtime-safe and will feel free to allocate internally.
    AudioThreadSafe,

    /// This source is as latent as reading memory and performing CPU work to decode.
    ///
    /// That is, it does not use the FS or other "devices".  It may or may not allocate, or perform other "memory-level"
    /// operations such as short-lived mutex acquisitions.
    Memory,

    /// This source is as latent as reading from the filesystem.
    Disk,
}

/// Describes the characteristics of a source.
///
/// This type is temporarily sealed, and so also seals the [SampleSource] trait.
#[derive(Debug, Clone)]
pub struct Descriptor {
    /// The sample rate.
    pub(crate) sample_rate: NonZeroU64,

    /// If known, the total duration of this source in samples.
    ///
    /// This must be set for sources which support seeking.
    pub(crate) duration: Option<u64>,

    /// What kind of seeking does this source support?
    pub(crate) seek_support: SeekSupport,

    /// How latent is this source?
    pub(crate) latency: Latency,

    pub(crate) channel_format: crate::channel_format::ChannelFormat,
}

impl Latency {
    pub(crate) fn is_audio_thread_safe(&self) -> bool {
        self == &Latency::AudioThreadSafe
    }
}
impl Descriptor {
    pub(crate) fn get_channel_count(&self) -> usize {
        self.channel_format.get_channel_count().get()
    }
}

/// Kinds of error a source might experience.
///
/// This module deals in an alternative error type because it is critical for some source implementations that they be
/// able to give errors back on the audio thread.
///
/// Synthizer has two kinds of errors [SampleSource]s may expose.
///
/// First is the non-allocating option: A `&'static str` prefix, and an inline string that lives on the stack.  It will
/// be rendered "My prefix: some data at (file:line)", where "some data" is truncated at an arbitrary
/// implementation-defined point.  Enough characters will always be available to display a full u64 value, e.g. errno.
/// No source or backtrace are available, but the file and line number are always present since capturing these doesn't
/// allocate.
///
/// Second is the allocating case, which takes any error and forwards to it directly.  This can handle and perfectly
/// preserve anything, but at the cost of having to allocate on error, thus making the source unsuitable for the audio
/// thread.  Sources which use this must not claim to be [Latency::AudioThreadSafe].  Synthizer does not currently
/// validate this is the case, but may panic in future if such an error is constructed on an audio thread.
#[derive(Debug)]
pub struct SampleSourceError {
    kind: SampleSourceErrorKind,
}

const SAMPLE_SOURCE_ERROR_DATA_LENGTH: usize = 64;

#[derive(Debug)]
enum SampleSourceErrorKind {
    /// This is an inline error, which will be of the format "{prefix}: {data} at (file:line)".
    ///
    /// Data is truncated as necessary.
    Inline {
        prefix: &'static str,
        message: Option<arrayvec::ArrayString<SAMPLE_SOURCE_ERROR_DATA_LENGTH>>,
        location: &'static std::panic::Location<'static>,
        truncated: bool,
    },

    Allocated(Box<dyn std::error::Error + Send + Sync>),
}

impl SampleSourceError {
    pub fn new_stack(prefix: &'static str, message: Option<&str>) -> SampleSourceError {
        let location = std::panic::Location::caller();
        let mut truncated = false;

        let message = message.map(|msg| {
            let mut message_av = arrayvec::ArrayString::<SAMPLE_SOURCE_ERROR_DATA_LENGTH>::new();
            // Apparently there is no good way of doing this with either built-in `&str` or `ArrayString`.  This is
            // surprising; I assume I am missing something; this should be fast enough in any case.
            for c in msg.chars() {
                if message_av.try_push(c).is_err() {
                    truncated = true;
                    break;
                }
            }

            message_av
        });

        let kind = SampleSourceErrorKind::Inline {
            prefix,
            message,
            truncated,
            location,
        };
        SampleSourceError { kind }
    }

    pub fn new_boxed(err: Box<dyn std::error::Error + Send + Sync + 'static>) -> SampleSourceError {
        SampleSourceError {
            kind: SampleSourceErrorKind::Allocated(err),
        }
    }
}

impl std::fmt::Display for SampleSourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            SampleSourceErrorKind::Allocated(e) => write!(f, "{}", e),
            SampleSourceErrorKind::Inline {
                prefix,
                message,
                location,
                truncated,
            } => {
                write!(f, "{prefix}")?;
                if let Some(msg) = message.as_ref() {
                    let elipsis = if *truncated { "..." } else { "" };
                    write!(f, ": {msg}{elipsis}")?;
                }

                write!(f, " at ({}:{})", location.file(), location.line())
            }
        }
    }
}

impl std::error::Error for SampleSourceError {
    fn cause(&self) -> Option<&dyn std::error::Error> {
        match &self.kind {
            SampleSourceErrorKind::Allocated(e) => Some(&**e),
            SampleSourceErrorKind::Inline { .. } => None,
        }
    }
}

impl From<&'static str> for SampleSourceError {
    fn from(value: &'static str) -> Self {
        Self::new_stack(value, None)
    }
}

/// Trait representing something which may yield samples.
///
/// This is how audio enters Synthizer.  Helper methods in this module can make sources from various kinds of things.
/// See, for example, [from_vec_f32].
///
/// Any method on this trait may be called from an audio thread if and only if the source claims that it only uses the
/// CPU.  As reiterated a few times in this documentation, be 100% sure capabilities are accurate.
///
/// This trait is temporarily sealed, because [Descriptor] is sealed.  The plan is to lift this restriction in the near
/// future, but it is important that we are sure we have things right so for now you cannot implement custom sources.
pub trait SampleSource: 'static + Send + Sync {
    /// Get the descriptor describing this source.
    ///
    /// Called exactly once only before any source processing takes place.  This is not fallible and should not block;
    /// sources should do the work of figuring out their descriptors as part of (possibly fallible) construction.
    fn get_descriptor(&self) -> Descriptor;

    /// Fill the provided buffer with as many samples as possible.
    ///
    /// The passed-in slice is *not* zeroed and is always at least one frame of audio data (that is, a nonzero multiple
    /// of the channel count).
    ///
    /// As with [std::io], returning `Ok(0)` means end.  Synthizer will never again call this function without first
    /// seeking once it signals the end, and will not seek if the source does not claim seeking is possible.
    ///
    /// Should return the number of *frames* written.
    fn read_samples(&mut self, destination: &mut [f32]) -> Result<u64, SampleSourceError>;

    /// Return true if this source can never again return samples, no matter what.
    ///
    ///  This happens primarily when a source encounters a fatal error from which it cannot recover, and will result in
    ///  the source never again having any method called on it.  Sources which are at the end may return true here for a
    ///  nice optimization, but if and only if they cannot seek.  This returning true is exactly equivalent to promising
    ///  that no matter what happens, [SampleSource::read_samples] will never again return any data.
    fn is_permanently_finished(&mut self) -> bool {
        false
    }

    /// Seek to the given sample.
    ///
    /// - If no seek support is signalled, this function is never called.
    /// - If [SeekSupport::ToBeginning] is specified, this function will only be called with 0.
    /// - Otherwise, this function may be called with any value `0..descriptor.duration_in_samples` if the duration is
    ///   known, or if not any u64 value.
    ///
    /// Inaccurate sources should always seek before the provided position, not after.  Sources which do not know their
    /// duration should make a best effort to seek to the end rather than erroring.
    fn seek(&mut self, position_in_frames: u64) -> Result<(), SampleSourceError>;
}

impl<T: SampleSource + ?Sized> SampleSource for Box<T> {
    fn get_descriptor(&self) -> Descriptor {
        (**self).get_descriptor()
    }

    fn is_permanently_finished(&mut self) -> bool {
        (**self).is_permanently_finished()
    }

    fn read_samples(&mut self, destination: &mut [f32]) -> Result<u64, SampleSourceError> {
        (**self).read_samples(destination)
    }

    fn seek(&mut self, position_in_frames: u64) -> Result<(), SampleSourceError> {
        (**self).seek(position_in_frames)
    }
}
