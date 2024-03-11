use std::fs::File;
use std::panic as stdp;
use std::path::Path;
use std::sync::Mutex;

use super::protocol::*;

/// Installs a panic handler which will write the panic response from [super::protocol] to the file specified.
///
/// This is slightly fragile in the sense that it will not handle (for example) C segfaults.  It also won't handle
/// anything which puts the process in such a bad state that it can no longer use serde_yaml and write a file (which, is
/// standard syscalls and the allocator).
///
/// This creates the file up front and keeps it open by moving it into the panic handler.
pub fn install_panic_handler(response_destination: &Path) {
    // allows for the panic handler to close the file, by setting the guard to None.
    // This mutex ensures that we only run one panic handler total. The handler aborts explicitrly.  Being an option
    let file = Mutex::new(Some(
        File::create(response_destination).expect("Unable to open file for the panic handler"),
    ));

    let old_handler = stdp::take_hook();

    let handler = Box::new(move |p_info: &stdp::PanicInfo| {
        // This should always at least display something.
        old_handler(p_info);

        let mut file_guard = file.lock().unwrap();
        // We know this is Some because no other thread may acquire the lock first; the process is about to abort.
        let mut file = file_guard.take().unwrap();

        let response = SubprocessResponse {
            outcome: TestOutcome::Panicked(PanicOutcome {
                panic_info: p_info.to_string(),
                location: p_info.location().map(|x| x.to_string()),
                backtrace: std::backtrace::Backtrace::force_capture().to_string(),
            }),
        };

        // We will permit a double panic here.  If that happens, things are probably screwed up enough that it wasn't
        // going to go well anyway.
        serde_yaml::to_writer(&mut file, &response).expect("Should serialize and write");

        // Close the file.  We could in theory do this by passing the file directly to serde_yaml, but it is important
        // that this happen and being explicit ensures that we can't miss it.
        std::mem::drop(file);
        std::process::abort();
    });

    stdp::set_hook(handler);
}
