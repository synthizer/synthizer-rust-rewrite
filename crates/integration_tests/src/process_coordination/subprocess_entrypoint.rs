use super::protocol as proto;

/// "main" but for the subprocess.
///
/// Each subprocess runs one test then exits.  See the test_execution module in the root of the crate for what that
/// looks like.  This function sets up the environment for that, then delegates to it
///
/// We do the following things here:
///
/// - Set a panic handler which will write to "response-panic.json".
/// - Set a logger which will write to `TESTROOT/logs/level.txt`.
/// - Delegate to the runner at the root of the crate which has a clue on how to actually do the thing.
///
/// There's a bit of duplicate logic here for now because of how early this has to run.  There's a chicken-and-egg
/// problem where a full test context is needed to get all the info on the environment, but cannot be brought into being
/// until the panic and log handlers exist (consider: what if creating a server crashes?)
pub fn subprocess_entrypoint(args: &crate::cli_args::SubprocessArgs) {
    let args: super::protocol::SubprocessRequest = serde_json::from_str(&args.json)
        .expect("Since we serialized the payload, we should always be able to deserialize it");

    let environment = crate::environment::get_env();

    let artifacts_root = environment.temp_artifacts_dir.join(&args.test_name);

    super::panic_handler::install_panic_handler(
        &artifacts_root.join(crate::environment::RESPONSE_PANIC_FILE),
    );
    super::log_handler::install_log_handler(&artifacts_root);

    let response = match crate::test_runner::run_single_test_in_subprocess(&args.test_name) {
        Ok(r) => r,
        Err(e) => proto::SubprocessResponse {
            outcome: proto::TestOutcome::RunnerFailed(proto::RunnerFailedResponse {
                reason: e.to_string(),
            }),
        },
    };

    let out_file =
        std::fs::File::create(artifacts_root.join(crate::environment::RESPONSE_GOOD_FILE))
            .expect("Could not create response file");
    serde_json::to_writer(out_file, &response).expect("Could not write the JSON response");
}
