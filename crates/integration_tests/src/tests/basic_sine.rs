//! Set up a basic sine wave with our basic validators, and validate that that is in fact what comes out.
use anyhow::Result;

use synthizer as syz;

use crate::context::TestContext;
use crate::test_config::{TestConfig, TestConfigBuilder};

const FREQ: f64 = 300.0f64;
const ERROR_RANGE: f64 = 0.0001;

fn validate_sin(_ctx: &TestContext, frame: u64, frame_data: &[f32]) -> Result<(), String> {
    let time_secs = frame as f64 / syz::SR as f64;
    let expected_val = (2.0 * std::f64::consts::PI * time_secs * FREQ).sin();
    for (c, s) in frame_data.iter().copied().enumerate() {
        if (expected_val - (s as f64)).abs() > ERROR_RANGE {
            return Err(format!(
                "At frame {frame} channel {c}: found {s} but expected {expected_val}"
            ));
        }
    }

    Ok(())
}

fn basic_sine_config() -> TestConfig {
    TestConfigBuilder::default()
        .add_standard_validators()
        .add_validator(validate_sin)
        .build()
        .unwrap()
}

fn basic_sine(context: &mut TestContext) -> Result<()> {
    let sine = syz::nodes::TrigWaveformNode::new_sin(&context.server, FREQ)?;
    let ao = syz::nodes::AudioOutputNode::new(&context.server, syz::ChannelFormat::Stereo)?;
    context.server.connect(&sine, 0, &ao, 0)?;
    context.advance(std::time::Duration::from_secs(10))?;
    Ok(())
}

register_test!(basic_sine);