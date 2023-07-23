use synthizer_miniaudio::Error as MiniaudioError;

#[derive(Debug, derive_more::Display)]
enum ErrorPayload {
    #[display(fmt = "Audio backend error: {}", _0)]
    AudioBackend(MiniaudioError),
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

pub type Result<T, E = Error> = std::result::Result<T, E>;
