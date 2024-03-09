//! Knows how to run  a single test, including launching the subprocess.
use anyhow::{Context, Result};

use crate::process_coordination::protocol;

/// Executed in the subprocess.  Runs a single test start to finish by name, and returns the protocol response to pass
/// along.
pub fn run_single_test_in_subprocess(name: &str) -> Result<protocol::SubprocessResponse> {
    let test = crate::registry::get_tests()
        .find(|x| x.name() == name)
        .ok_or_else(|| anyhow::anyhow!("Could not find test {name}"))?;

    let config = (test.config_fn)();
    let mut context = crate::context::TestContext::from_config(name, config)?;
    let test_res = (test.run_fn)(&mut context);

    // If the test itself returned an error, that's a failure regardless of what validators might or might not say.
    if let Err(e) = test_res {
        return Ok(protocol::SubprocessResponse {
            outcome: protocol::TestOutcome::RunnerFailed(protocol::RunnerFailedResponse {
                reason: e.to_string(),
            }),
        });
    }

    let validator_failures = context
        .finalize_validators()
        .into_iter()
        .enumerate()
        .filter_map(|x| {
            Some(protocol::FailedValidatorEntry {
                index: x.0,
                payload: x.1.err()?,
            })
        })
        .collect::<Vec<_>>();

    // If any validator failed, tel the parent.
    if !validator_failures.is_empty() {
        return Ok(protocol::SubprocessResponse {
            outcome: protocol::TestOutcome::ValidatorsFailed(protocol::ValidatorsFailedResponse {
                entries: validator_failures,
            }),
        });
    }

    Ok(protocol::SubprocessResponse {
        outcome: protocol::TestOutcome::Passed,
    })
}

/// The half of the process running logic which runs in the parent.
///
/// This will:
///
/// - Clean any old artifacts directory.
/// - Run the test in a subprocess.
/// - If the test passes, clean the artifacts directory.
///
/// We want to clean the artifacts directory for passing tests because they can be incredibly huge when doing
/// determinism checking or fingerprinting.
pub fn run_single_test_in_parent(name: &str) -> Result<protocol::SubprocessResponse> {
    let test = crate::registry::get_tests()
        .find(|x| x.name() == name)
        .ok_or_else(|| anyhow::anyhow!("Unable to find a registry entry for test {name}"))?;
    let config = (test.config_fn)();

    let artifacts_directory = crate::environment::get_env().temp_artifacts_dir.join(name);

    match std::fs::remove_dir_all(&artifacts_directory) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            let ret = Err(anyhow::anyhow!(e));
            return ret.context(format!(
                "While trying to clear artifacts directory for {name}: {}",
                artifacts_directory.display()
            ));
        }
    }

    let outcome = crate::process_coordination::parent_process::parent_process(name)?;
    if outcome.outcome.is_passed() && !config.keep_artifacts_on_success {
        std::fs::remove_dir_all(&artifacts_directory).context(format!("While trying to clean up the artifacts directory after a successful test name={name}: {}", artifacts_directory.display()))?;
    }

    Ok(outcome)
}

/// Filter down the tests with the provided filter and run them all.  Then, report the outcome to stderr.
///
/// This function never returns, and exits the process with the appropriate error code.
pub fn run_tests(filter: &crate::cli_args::FilterArgs) {
    let mut all_passed = true;

    for test in crate::test_filtering::get_tests_filtered(filter) {
        let name = test.name();
        let res = run_single_test_in_parent(test.name())
            .expect("Running tests themselves should always work unless the harness is bugged or the environment is bad");
        let reported = crate::reporter::report_test(name, &(test.config_fn)(), &res.outcome);
        all_passed &= matches!(res.outcome, protocol::TestOutcome::Passed);
        eprintln!("{reported}");
    }

    let exit_code = (!all_passed) as _;
    std::process::exit(exit_code);
}
