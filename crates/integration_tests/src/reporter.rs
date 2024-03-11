//! Infrastructure to report the results of running a test.
//!
//! This is called from [crate::test_runner] to display the outputs of tests.
use std::fmt::{Result, Write};

use indenter::indented;

use crate::environment::get_env;
use crate::process_coordination::protocol as proto;

// Implementation: we have a root entrypoint at `report_test`, then we use indenter to bring everything together.
// Formatting here is to strings and so cannot fail, but unwrap is annoying so we put that behind a function and unwrap
// once at the top.
//
// The output string does not contain a newline, and this is handled by stripping at the top.  This lets us use writeln
// everywhere.

/// Report the outcome of a test.
///
/// Returns a string without a trailing newline.
pub fn report_test(
    test_name: &str,
    test_config: &crate::test_config::TestConfig,
    outcome: &proto::TestOutcome,
) -> String {
    let mut dest = String::new();
    report_test_fallible(&mut dest, test_name, test_config, outcome)
        .expect("This is formatting to strings and should never fail");

    // it's really hard to get newlines right, so we strip at the top.
    let Some(stripped) = dest.strip_suffix('\n') else {
        return dest;
    };
    stripped.to_string()
}

fn report_test_fallible(
    mut dest: &mut dyn Write,
    test_name: &str,
    test_config: &crate::test_config::TestConfig,
    outcome: &proto::TestOutcome,
) -> Result {
    write!(dest, "{test_name} ")?;

    match outcome {
        proto::TestOutcome::Passed => write!(dest, "passed")?,
        proto::TestOutcome::Panicked(p) => {
            writeln!(dest, "panicked")?;
            report_panic(&mut indented(&mut dest).with_str("  "), test_name, p)?;
        }
        proto::TestOutcome::RunnerFailed(r) => {
            writeln!(dest, "Runner Failed")?;
            report_runner_failed(&mut indented(&mut dest).with_str("  "), r)?;
        }
        proto::TestOutcome::ValidatorsFailed(v) => {
            writeln!(dest, "Validators failed")?;
            report_validators_failed(&mut indented(&mut dest).with_str("  "), test_config, v)?;
        }
    }

    if !outcome.is_passed() {
        // Mind the double spaces at the beginning of this string.
        writeln!(dest, "  More information may be available. Try cargo run --bin synthizer_integration_tests -- view-response {test_name}")?;
    }

    Ok(())
}

fn report_panic(dest: &mut dyn Write, test_name: &str, info: &proto::PanicOutcome) -> Result {
    let panic_resp = get_env().panic_response_file_for(test_name);
    let pan_info = &info.panic_info;
    let loc = info.location.as_deref().unwrap_or("UNAVAILABLE");
    writeln!(dest, "{pan_info}")?;
    writeln!(dest, "Location: {loc}")?;
    writeln!(dest, "NOTE: more info in {}", panic_resp.display())?;
    Ok(())
}

fn report_runner_failed(dest: &mut dyn Write, info: &proto::RunnerFailedResponse) -> Result {
    writeln!(dest, "Reason: {}", info.reason)
}

fn report_validators_failed(
    mut dest: &mut dyn Write,
    test_config: &crate::test_config::TestConfig,
    info: &proto::ValidatorsFailedResponse,
) -> Result {
    writeln!(dest, "{} validators have failed", info.entries.len())?;

    for v in info.entries.iter() {
        let mut ind_fmt = indented(&mut dest);
        let tag = test_config.validators[v.index].get_tag();
        writeln!(&mut ind_fmt, "Validator {} (a {tag}): ", v.index)?;
        writeln!(ind_fmt.with_str("  "), "{}", v.payload)?;
    }
    Ok(())
}
