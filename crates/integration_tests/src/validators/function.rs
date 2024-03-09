//! Implements [super::ValidatorBuilder] for functions.
use std::panic::Location;

use crate::context::TestContext;

enum FunctionValidator<F> {
    KeepGoing(F),
    Failed(&'static Location<'static>, String),
}

impl<F> super::Validator for FunctionValidator<F>
where
    F: FnMut(&TestContext, &[f32]) -> Result<(), String> + Send + Sync + 'static,
{
    fn validate_frame(
        &mut self,
        context: &TestContext,
        location: &'static Location<'static>,
        frame: &[f32],
    ) {
        if let FunctionValidator::KeepGoing(ref mut cb) = self {
            if let Err(msg) = cb(context, frame) {
                *self = FunctionValidator::Failed(location, msg);
            }
        }
    }

    fn finalize(&mut self, _context: &TestContext) -> Result<(), super::ValidatorFailure> {
        match self {
            FunctionValidator::KeepGoing(_) => Ok(()),
            FunctionValidator::Failed(loc, msg) => Err(super::ValidatorFailure::SimpleMessage(
                format!("{}: {}", loc, msg),
            )),
        }
    }
}

impl<F> super::IntoValidator for F
where
    F: FnMut(&TestContext, &[f32]) -> Result<(), String> + Send + Sync + 'static,
{
    fn build_validator(self: Box<Self>, _context: &TestContext) -> Box<dyn super::Validator> {
        Box::new(FunctionValidator::KeepGoing(*self))
    }

    fn get_tag(&self) -> &str {
        "Closure"
    }
}
