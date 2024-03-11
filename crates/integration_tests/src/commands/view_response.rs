use crate::cli_args::*;

pub fn view_response(_top_args: &CliArgs, cmd_args: &ViewResponseArgs) {
    let env = crate::environment::get_env();

    if crate::registry::get_test_by_name(&cmd_args.test_name).is_none() {
        panic!("{} is not a valid, registered test", cmd_args.test_name);
    };

    let artifacts_dir = env.artifacts_dir_for(&cmd_args.test_name);
    if !artifacts_dir.exists() {
        panic!(
            "{}: no artifacts directory found. Tried {}. This probably means the test passed.",
            cmd_args.test_name,
            artifacts_dir.display()
        );
    }

    let possibilities = [
        env.panic_response_file_for(&cmd_args.test_name),
        env.good_response_file_for(&cmd_args.test_name),
    ];

    let mut response: Option<String> = None;
    for p in possibilities {
        match std::fs::read(&p) {
            Ok(r) => {
                response = Some(
                    String::from_utf8(r).expect("This is a JSON file and should always be UTF-8"),
                );
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                panic!("While trying to open path {}: {}", p.display(), e);
            }
        }
    }

    let response = response.expect(
        "Unable to find a response file in the artifacts directory. Manual examination is required",
    );

    println!("Response for {}", cmd_args.test_name);
    println!("{response}");
}
