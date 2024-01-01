mod symphonia_impl;

use std::fs::File;
use std::io::Cursor;

use crate::sample_sources::{SampleSource, SampleSourceError};

/// This sealed trait is how one gets to a SymphoniaWrapper.
mod sealed {
    use super::*;
    use symphonia_impl::*;

    pub struct Decoder(pub(super) SymphoniaWrapper);

    pub trait IntoDecoder {
        fn into_decoder(self) -> Result<Decoder, SampleSourceError>;
    }
}

use sealed::*;

/// Get a source implementation for a stream of encoded bytes.
///
/// This currently produces sources which are not realtime-safe, and which do not support precise seeking.  The backing
/// implementation is Symphonia but is subject to change, in particular because we will propbably carve out subsets with
/// specific libraries in order to do better for formats where we can be realtime-safe.
///
/// The generic parameter is using a sealed trait with a fixed set of impls.  You may pass:
///
/// - A file.
/// - `std::io::Cursor` where the wrapped type is `AsRef<[u8]>`.
///
/// Other types are not possible until this implementation matures, but the goal is to eventually open this up to any
/// `Read + Seek`, not just a set of fixed internal types.
pub fn create_encoded_source<S: IntoDecoder>(
    source: S,
) -> Result<Box<dyn SampleSource>, crate::error::Error> {
    Ok(Box::new(source.into_decoder()?.0))
}

impl IntoDecoder for File {
    fn into_decoder(self) -> Result<Decoder, SampleSourceError> {
        Ok(Decoder(
            symphonia_impl::build_symphonia(self)
                .map_err(|e| SampleSourceError::new_boxed(Box::new(e)))?,
        ))
    }
}

impl<T> IntoDecoder for Cursor<T>
where
    T: AsRef<[u8]> + Send + Sync + 'static,
{
    fn into_decoder(self) -> Result<Decoder, SampleSourceError> {
        Ok(Decoder(
            symphonia_impl::build_symphonia(self)
                .map_err(|e| SampleSourceError::new_boxed(Box::new(e)))?,
        ))
    }
}
