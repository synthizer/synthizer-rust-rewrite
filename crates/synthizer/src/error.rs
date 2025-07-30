use std::borrow::Cow;
use std::convert::Infallible;

use crate::cpal_device::AudioDeviceError;
use crate::loop_spec::LoopSpecError;

#[derive(Debug, derive_more::Display, derive_more::IsVariant)]
enum ErrorPayload {
    #[display("Audio backend error: {}", _0)]
    AudioBackend(AudioDeviceError),

    #[display("Loop specification error: {}", _0)]
    LoopSpec(crate::loop_spec::LoopSpecError),

    #[display("Validation error: {}", _0)]
    Validation(Cow<'static, str>),

    #[display("Symphonia error: {}", _0)]
    Symphonia(symphonia::core::errors::Error),

    #[display("Resampler error: {}", _0)]
    Resampler(crate::resampling::ResamplingError),

    #[display("Channel closed")]
    ChannelClosed,

    #[display("Io: {}", _0)]
    Io(std::io::Error),
}

#[derive(Debug, thiserror::Error)]
#[error("{payload}")]
pub struct Error {
    payload: ErrorPayload,
}

macro_rules! conv {
    ($variant: ident, $from_err: path) => {
        impl From<$from_err> for Error {
            fn from(value: $from_err) -> Error {
                Error {
                    payload: ErrorPayload::$variant(value),
                }
            }
        }
    };
}

conv!(AudioBackend, AudioDeviceError);
conv!(LoopSpec, LoopSpecError);
conv!(Symphonia, symphonia::core::errors::Error);
conv!(Resampler, crate::resampling::ResamplingError);
conv!(Io, std::io::Error);

impl<T> From<crossbeam::channel::SendError<T>> for Error {
    fn from(_: crossbeam::channel::SendError<T>) -> Error {
        Error {
            payload: ErrorPayload::ChannelClosed,
        }
    }
}

impl From<Infallible> for Error {
    fn from(inf: Infallible) -> Error {
        match inf {}
    }
}

impl Error {
    /// Create a validation error guaranteed to be backed by a static string.
    ///
    /// This is useful because it may be called in realtime contexts and will not accidentally allocate.
    pub(crate) fn new_validation_static(message: &'static str) -> Self {
        Self {
            payload: ErrorPayload::Validation(Cow::Borrowed(message)),
        }
    }

    /// Create a validation error which will borrow the input string when possible, or otherwise take an allocated
    /// string on the heap.
    pub(crate) fn new_validation_cow<S: Into<Cow<'static, str>>>(message: S) -> Self {
        Self {
            payload: ErrorPayload::Validation(message.into()),
        }
    }
    /// Does this error represent an invalid [crate::LoopSpec]?
    pub fn is_invalid_loop(&self) -> bool {
        self.payload.is_loop_spec()
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
