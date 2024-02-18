use std::borrow::Cow;

use synthizer_miniaudio::Error as MiniaudioError;

use crate::loop_spec::LoopSpecError;
use crate::sample_sources::SampleSourceError;

#[derive(Debug, derive_more::Display, derive_more::IsVariant)]
enum ErrorPayload {
    #[display(fmt = "Audio backend error: {}", _0)]
    AudioBackend(MiniaudioError),

    #[display(fmt = "Sample source error: {}", _0)]
    SampleSource(SampleSourceError),

    #[display(fmt = "Loop specification error: {}", _0)]
    LoopSpec(crate::loop_spec::LoopSpecError),

    #[display(fmt = "Validation error: {}", _0)]
    Validation(Cow<'static, str>),
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

conv!(AudioBackend, MiniaudioError);
conv!(SampleSource, SampleSourceError);
conv!(LoopSpec, LoopSpecError);

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
