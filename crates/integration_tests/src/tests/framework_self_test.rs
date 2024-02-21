//! This test does nothing.
//!
//! It exists so that there is always at least one test to run, as well as having somethingwith which we can test the
//! test harness itself.
use crate::context::TestContext;
use crate::test_config::{TestConfig, TestConfigBuilder};
use anyhow::Result;

fn framework_self_test_config() -> TestConfig {
    TestConfigBuilder::default()
        .add_standard_validators()
        .keep_artifacts_on_success(true)
        .build()
        .unwrap()
}

fn framework_self_test(_context: &mut TestContext) -> Result<()> {
    // We just do a bunch of stuff which writes to the files. We have to check by hand.  It's not worth making a
    // recursive testing framework framework.
    println!("This is to stdout");
    eprintln!("And this is to stderr");
    log::error!("An error");
    log::warn!("A warning");
    log::info!("Some info");
    log::debug!("Debugging");
    log::trace!("tracing now");
    Ok(())
}

register_test!(framework_self_test);
