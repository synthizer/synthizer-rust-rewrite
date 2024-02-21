#![allow(dead_code)]
//!SSynthizer integration testing.
//!
//! Synthizer is an audio library which means it has rather interesting properties such as float non-determinism, and
//! other things such as not knowing what audio will come out (consider a reverb algorithm fed by a file).  To that end,
//! traditional testing strategies don't really work so well.  They can be applied to the logic, but not easily to the
//! math.  On top of all of that, an audio device is not always available, e.g. in CI.  This crate is a binary target
//! that tries to address these issues.
//!
//! A test here works as follows:
//!
//! - The function gets a [context::TestContext] which contains among other things a Synthizer server configured to
//!   output inline.
//! - The test sets things up, then drives the context forward by calling `.run_until` with one of a few different types
//!   to push the test forward.
//! - The test is separately registered via inventory, which isn't as convenient as the Rust test attribute but is the
//!   only way to let us do configuration.
//! - When a test is registerd, validators are connected to it, and compare the sequence to the validator.
//!
//! Some example validators are these:
//!
//! - Any `Fn(u64, &mut[f64])` and other similar function signatures capable of generating frames of data from a sample
//!   index can be used to check exactly.  For example, this can check basic trigonometric waveforms.
//! - The [validators::golden] module can be used to take a fingerprint which is saved to the `golden` directory, and
//!   then compared on the next test run.  See module documentation for how fingerprinting works.
//!
//! Multiple validators may be added to a test.
//!
//! In terms of the internals, each test is run in a subprocess in parallel.  These subprocesses write a bunch of output
//! to `target/integration_tests_artifacts`.  The parent harness scrapes these output directories to determine what
//! happened, and other fiels such as the actual wave output are written there for examination.
//!
//! The following directories are involved in all of this:
//!
//! - `crates/integration_tests/golden` will contain generated fingerprints for golden master tests.
//! - `crates/integration_tests/assets` contains test files, for example small wave files, which can be run through
//!   tests (committed directly to the repo because GitHub LFS pricing isn't advantageous, so this directory must be
//!   kept small).
//! - `target/integration_tests_artifacts` contains stdout/stderr of tests, their logs, wave files representing their
//!   outputs, and (where applicable) proposed fingerprints.
//!
//! Under these directories, there are a number of as-needed top-level files, and then each test gets a `test_name`
//! subdirectory where the test specific information is stored.  Tests in this crate created with the registration macro
//! default to `module.path.to.test.run_fn` where `run_fn` is the user-supplied name.  Replacing :: with . makes this FS
//! compatible, and including the module allows uniqueness without reading the entire crate.  To learn how to write a
//! test, see [crate::tests::framework_self_test], which is a Synthizer-free test used to hand-check that the
//! integration tests themselves work.
//!
//! This must be built and run with `cargo run` in the Synthizer workspace, and cannot run outside Synthizer's workspace
//! because of the above directories.  It is also written assuming `panic = abort`, configured in the workspace root, so
//! that we never fail to catch a panic on a thread we're not monitoring.
#[macro_use]
mod registration_macro;

mod cli_args;
mod commands;
mod context;
mod environment;
mod process_coordination;
mod registry;
mod test_config;
mod test_filtering;
mod test_runner;
mod tests;
mod validators;

fn main() {
    use clap::Parser;

    let args = cli_args::CliArgs::parse();
    commands::dispatch_command(args);
}
