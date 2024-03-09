//! Defines the protocol (serde types) which are used to talk between subprocesses.
//!
//! Each test corresponds to one subprocess invocation which receives a JSON blob on the command line.  Tests are then
//! expected to write a `response.json` in their artifacts directory saying how the test went, or a
//! `response_panic.json` if they panic (the latter is a different file so that the panic handler may hold the file open
//! from process start).
//!
//! This file just defines the types, which are consumed by other modules.
use serde::{Deserialize, Serialize};

/// Protocol to run a test.
///
/// the environment overall is recomputed by the runner because the subprocess shares the environment of the parent
/// process, so this need only forward information about the test to run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubprocessRequest {
    /// the test we are to run.
    ///
    /// This is then looked up in the registry.
    pub test_name: String,

    /// The number of the pass.
    ///
    /// Some tests are run more than once, primarily to validate determinism.  We almost never run one more than twice.
    /// For single-run tests this is always 0, but for the nth run of multi-run tests it is `n - 1`.
    pub pass: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubprocessResponse {
    pub outcome: TestOutcome,
}

/// The outcome of a test.
#[derive(Clone, Debug, Serialize, Deserialize, derive_more::IsVariant)]
pub enum TestOutcome {
    /// Test passed this time.  If there are further runs, cancel them.
    Passed,

    /// The test runner function failed.
    RunnerFailed(RunnerFailedResponse),

    /// The test was configured to need more than one pass and does not yet know what the outcome is.
    ///
    /// If this is the nth pass of an n-pass test, this is a failure.
    Indeterminate,

    /// The test failed because some validator did.
    ValidatorsFailed(ValidatorsFailedResponse),

    /// The process panicked.  Could be an assert or an actual problem.
    Panicked(PanicOutcome),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FailedValidatorEntry {
    /// The index of the failed validator.
    ///
    /// because config functions are deterministic, it's possible to match these up between the child and parent
    /// processes.
    pub index: usize,

    pub payload: crate::validators::ValidatorFailure,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidatorsFailedResponse {
    pub entries: Vec<FailedValidatorEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PanicOutcome {
    /// Display impl result on PanicInfo.
    pub panic_info: String,

    /// Location, if present.
    pub location: Option<String>,

    /// Stringified backtrace.
    pub backtrace: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RunnerFailedResponse {
    pub reason: String,
}
