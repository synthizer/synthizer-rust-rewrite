//! Implements [super::ValidatorBuilder] for functions.
use std::panic::Location;

use crate::context::TestContext;

enum FunctionValidator<F> {
    KeepGoing { callback: F, frame_time: u64 },
    Failed(&'static Location<'static>, String),
}

impl<F> super::Validator for FunctionValidator<F>
where
    F: FnMut(&TestContext, u64, &[f32]) -> Result<(), String> + Send + Sync + 'static,
{
    fn validate_frame(
        &mut self,
        context: &TestContext,
        location: &'static Location<'static>,
        frame: &[f32],
    ) {
        // We must pull out the failure information and assign it at the end because matching self is matrching over a
        // mutable reference.
        let maybe_failed = if let FunctionValidator::KeepGoing {
            callback,
            frame_time,
        } = self
        {
            if let Err(msg) = callback(context, *frame_time, frame) {
                Some((location, msg))
            } else {
                *frame_time += 1;
                None
            }
        } else {
            None
        };

        if let Some((loc, msg)) = maybe_failed {
            *self = FunctionValidator::Failed(loc, msg);
        }
    }

    fn finalize(&mut self, _context: &TestContext) -> Result<(), super::ValidatorFailure> {
        match self {
            FunctionValidator::KeepGoing { .. } => Ok(()),
            FunctionValidator::Failed(loc, msg) => Err(super::ValidatorFailure::SimpleMessage(
                format!("{}: {}", loc, msg),
            )),
        }
    }
}

impl<F> super::IntoValidator for F
where
    F: FnMut(&TestContext, u64, &[f32]) -> Result<(), String> + Send + Sync + 'static,
{
    fn build_validator(self: Box<Self>, _context: &TestContext) -> Box<dyn super::Validator> {
        Box::new(FunctionValidator::KeepGoing {
            callback: *self,
            frame_time: 0,
        })
    }

    fn get_tag(&self) -> &str {
        "Closure"
    }
}
