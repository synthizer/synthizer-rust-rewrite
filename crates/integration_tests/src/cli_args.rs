//! Definition of the Clap command line.
//!
//! This is big; we opt to pull it out.
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
pub struct CliArgs {
    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run tests.
    Run(RunArgs),

    /// List all tests.
    List(ListArgs),

    /// Private command used as an entrypoint to subprocesses.
    SubprocessEntryPoint(SubprocessArgs),
}

#[derive(Debug, Parser)]
pub struct FilterArgs {
    /// If specified, filter tests with this glob pattern.
    pub pattern: Option<String>,
}

#[derive(Debug, Parser)]
pub struct RunArgs {
    #[command(flatten)]
    pub filter: FilterArgs,
}

#[derive(Debug, Parser)]
pub struct SubprocessArgs {
    /// The test runner logic passes information to the subprocess as an arbitrary JSON string serialized with serde.
    ///
    /// See the `runner` module for more.
    pub json: String,
}

/// List all tests, optionally constrained by a filter.
#[derive(Debug, Parser)]
pub struct ListArgs {
    #[command(flatten)]
    pub filter: FilterArgs,
}
