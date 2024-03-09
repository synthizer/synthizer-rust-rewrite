mod list;
mod run;
mod subprocess_entry_point;
mod view_response;

use crate::cli_args;

/// Figure out what command to run, then run it.
pub fn dispatch_command(args: cli_args::CliArgs) {
    match &args.command {
        cli_args::Command::List(l) => list::list(&args, l),
        cli_args::Command::ViewResponse(r) => view_response::view_response(&args, r),
        cli_args::Command::Run(r) => run::run(&args, r),
        cli_args::Command::SubprocessEntryPoint(sp) => {
            subprocess_entry_point::subprocess_entry_point(&args, sp)
        }
    }
}
