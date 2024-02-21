use anyhow::Result;

use synthizer as syz;

use crate::test_config::TestConfig;
use crate::validators::Validator;

pub struct TestContext {
    pub test_name: String,
    pub channel_format: syz::ChannelFormat,
    pub validators: Vec<Box<dyn Validator>>,
}

impl TestContext {
    pub fn from_config(test_name: &str, config: TestConfig) -> Result<TestContext> {
        let mut ret = Self {
            channel_format: syz::ChannelFormat::Stereo,
            test_name: test_name.to_string(),
            validators: vec![],
        };

        let validators = config
            .validators
            .into_iter()
            .map(|x| x.build_validator(&ret))
            .collect::<Vec<_>>();
        ret.validators = validators;

        Ok(ret)
    }

    /// Get the outcomes of all validators.
    pub fn finalize_validators(&mut self) -> Vec<Result<(), crate::validators::ValidatorFailure>> {
        // We need to be able to let the validators see the context, so take the list of validators out, then put it
        // back.
        let mut validators = std::mem::take(&mut self.validators);

        let ret = validators.iter_mut().map(|x| x.finalize(self)).collect();
        self.validators = validators;
        ret
    }
}
