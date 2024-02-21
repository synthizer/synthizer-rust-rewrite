use crate::cli_args::{CliArgs, SubprocessArgs};

pub fn subprocess_entry_point(_top_args: &CliArgs, subproc_args: &SubprocessArgs) {
    crate::process_coordination::subprocess_entrypoint::subprocess_entrypoint(subproc_args);
}
