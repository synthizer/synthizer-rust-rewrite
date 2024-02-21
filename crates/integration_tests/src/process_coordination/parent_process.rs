use super::protocol;

use anyhow::{Context, Result};

/// This function runs in the parent process.  It will parse a run for a given test and return the result if possibl.
///
/// Cleanup of previous runs, etc. all happen one level up in the harness, since that is what has the global view of
/// passes and other features which may wish to keep artifacts around.
pub fn parent_process(test_name: &str) -> Result<protocol::SubprocessResponse> {
    let request = protocol::SubprocessRequest {
        test_name: test_name.to_string(),
        pass: 0,
    };

    let testroot = crate::environment::get_env()
        .temp_artifacts_dir
        .join(test_name);
    let ecode = super::exec_subprocess::exec_subprocess(&testroot, &request)?;

    let (response_file_name, maybe_panic) = if ecode.success() {
        (crate::environment::RESPONSE_GOOD_FILE, false)
    } else {
        (crate::environment::RESPONSE_PANIC_FILE, true)
    };

    let mut maybe_resp_file =
        std::fs::File::open(testroot.join(response_file_name)).map_err(|e| anyhow::anyhow!(e));
    if maybe_panic {
        maybe_resp_file = maybe_resp_file.context(
            "The subprocess exited abnormally, but there was no response-panic.json present",
        );
    }
    let resp_file = maybe_resp_file?;

    Ok(serde_json::from_reader(resp_file)?)
}
