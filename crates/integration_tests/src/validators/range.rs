use std::panic::Location;

use super::*;

/// A [IntoValidator] which validates that audio never leaves the range `-1.0..=1.0`.
///
/// Also detects NaN.
pub struct RangeValidator;

#[derive(Debug, derive_more::IsVariant)]
enum State {
    Good,

    Failed {
        index: u64,
        location: &'static Location<'static>,
        value: f64,
        channel: u64,
    },
}

/// The piece that's behind the box.
struct RangeImpl {
    current_index: u64,
    state: State,
}

impl Validator for RangeImpl {
    fn validate_frame(
        &mut self,
        _context: &TestContext,
        location: &'static Location<'static>,
        frame: &[f64],
    ) {
        if !self.state.is_good() {
            return;
        }

        for (c, s) in frame.iter().copied().enumerate() {
            if !(-1.0..=1.0).contains(&s) {
                self.state = State::Failed {
                    index: self.current_index,
                    location,
                    value: s,
                    channel: c as u64,
                };
                return;
            }
        }

        self.current_index += 1;
    }

    fn finalize(&mut self, _context: &TestContext) -> Result<(), ValidatorFailure> {
        if let State::Failed {
            index,
            location,
            value,
            channel,
        } = &self.state
        {
            return Err(ValidatorFailure::SimpleMessage(format!(
                "{location}: out of range at frame {index} with value {value} on channel {channel}"
            )));
        }

        Ok(())
    }
}

impl IntoValidator for RangeValidator {
    fn build_validator(self: Box<Self>, _context: &TestContext) -> Box<dyn Validator> {
        Box::new(RangeImpl {
            state: State::Good,
            current_index: 0,
        })
    }

    fn get_tag(&self) -> &str {
        "RangeValidator"
    }
}
