use crate::validators::IntoValidator;

/// Configuration for a test.
#[derive(derive_builder::Builder)]
#[builder(pattern = "owned")]
pub struct TestConfig {
    #[builder(setter(custom))]
    pub validators: Vec<Box<dyn IntoValidator>>,

    /// If set, keep the artifact directory for this test even on successes.
    ///
    /// This is used basically only for the self test where we have a test that does nothing to make sure the framework
    /// works.  Other tests shouldn't have it, save while debugging weirdness, since the framework writes large files.
    pub keep_artifacts_on_success: bool,
}

impl TestConfigBuilder {
    pub fn add_validator<V: IntoValidator>(mut self, validator: V) -> Self {
        self.validators
            .get_or_insert_with(Vec::new)
            .push(Box::new(validator));
        self
    }

    /// Push a set of standard validators.  Checks that:
    ///
    /// - All generated samples are in-range for audio.
    ///
    /// Note that this can be quite slow; if an added test is taking too long consider adding a subset manually.  In
    /// particular, this (will eventually) always ask for determinism (unneeded if the exact output is already known,
    /// and runs the test at least twice) and golden testing.
    pub fn add_standard_validators(self) -> Self {
        self.add_validator(crate::validators::RangeValidator)
    }
}
