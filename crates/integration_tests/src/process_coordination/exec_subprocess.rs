use std::fs::File;
use std::path::Path;
use std::process as proc;

use anyhow::Result;

/// Executes the subprocess given a root for the test and the payload to pass to the subprocess.
///
/// Doesn't parse responses.  This just builds the command, which is sadly a bit involved.
///
/// When this function returns `ROOT/stdio/std{out,err}.txt` contain records of the process's stdout and stderr.
pub fn exec_subprocess(
    root: &Path,
    payload: &super::protocol::SubprocessRequest,
) -> Result<proc::ExitStatus> {
    let json = serde_json::to_string(payload)?;

    let stdio_root = root.join("stdio");
    std::fs::create_dir_all(&stdio_root)?;
    let stdout_file = File::create(stdio_root.join("stdout.txt"))?;
    let stderr_file = File::create(stdio_root.join("stderr.txt"))?;

    let selfpath = std::env::current_exe()?;

    let mut command = proc::Command::new(selfpath);
    command
        .arg("subprocess-entry-point")
        .arg(&json)
        .stdin(proc::Stdio::null())
        .stdout(stdout_file)
        .stderr(stderr_file);
    let mut child = command.spawn()?;

    let res = child.wait()?;
    Ok(res)
}
