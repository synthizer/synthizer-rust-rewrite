use crate::cli_args::{CliArgs, RunArgs};

pub fn run(_top_args: &CliArgs, run_args: &RunArgs) {
    crate::test_runner::run_tests(&run_args.filter);
}
