mod function;
mod golden;
mod range;

use std::panic::Location;

use serde::{Deserialize, Serialize};

use crate::context::TestContext;

pub use range::*;

/// Reasons a validator may fail.
///
/// Some validators are able to provide more semantic information than a string. This enum allows capturing that.
#[derive(Clone, Debug, derive_more::Display, Serialize, Deserialize)]
pub enum ValidatorFailure {
    #[display(fmt = "{}", _0)]
    SimpleMessage(String),
}

/// Something which can validate a realtime sequence as it is generated.
///
/// Validators should not stop early or panic.  By proceeding to the end, it is possible to get all weirdness for
/// examination.
pub trait Validator: Send + Sync + 'static {
    /// Move this validator forward by one audio frame.
    fn validate_frame(
        &mut self,
        context: &TestContext,
        location: &'static Location<'static>,
        frame: &[f32],
    );

    /// If this validator has failed, return `Err` explaining why.
    ///
    /// Validators are permitted to write to disk here, e.g. for [golden].
    fn finalize(&mut self, context: &TestContext) -> Result<(), ValidatorFailure>;

    /// Batched validator tick.
    ///
    /// This method is provided by default.  It will receive a context and a slice whose length is a multiple of the
    /// channel count in the context, and must split that slice into subslices.  The point of this function is that the
    /// single-frame version is expensive behind a box.
    fn validate_batched(
        &mut self,
        context: &TestContext,
        location: &'static Location<'static>,
        data: &[f32],
    ) {
        let chancount = context.channel_format.get_channel_count().get();

        let frames = data.len() / chancount;
        assert_eq!(frames * chancount, data.len());

        for frame in data.chunks_exact(chancount) {
            self.validate_frame(context, location, frame);
        }
    }
}

/// Trait representing something which may build a validator.  An alternative to `Into` for `Box<dyn Validator>`.
///
/// This trait exists so that configuration may be built in a generic way.  The various validator builders implement it
/// and build their validators into boxes.
///
/// In addition to the specific structs in this crate, this trait is also implemented for `FnMut(u64, &TestContext,
/// &[f64]) ->Result<(), String>`, which will stop the first time the given callback returns an error.  This is useful
/// when the exact sequence is known to some tolerance.
pub trait IntoValidator: 'static {
    fn build_validator(self: Box<Self>, context: &TestContext) -> Box<dyn Validator>;

    /// The tag of a validator is the name, e.g. "golden", "closure".
    ///
    /// Used for printing test results.
    fn get_tag(&self) -> &str;
}
