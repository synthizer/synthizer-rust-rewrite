#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Audio backend error: {}", message)]
    AudioBackend { message: String },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
