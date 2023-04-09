use std::borrow::Cow;

#[derive(thiserror::Error, Debug)]
#[error("Audio backend error: {}", message)]
pub struct Error {
    message: Cow<'static, str>,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    pub(crate) fn new<T>(msg: T) -> Error
    where
        Cow<'static, str>: From<T>,
    {
        Error {
            message: msg.into(),
        }
    }

    pub(crate) fn clone_internal(&self) -> Self {
        Self {
            message: self.message.clone(),
        }
    }
}
