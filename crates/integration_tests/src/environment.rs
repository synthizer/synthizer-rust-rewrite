//! Module getting the environment information for tests.
//!
//! The values here are cached the first time they are received, then returned to the user.  See the crate-level
//! documentation for information on our testing strategy and what the various things here mean.
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Environment<T> {
    pub golden_dir: T,
    pub temp_artifacts_dir: T,
    pub assets_dir: T,
}

fn compute_env() -> Environment<PathBuf> {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let manifest = PathBuf::from(manifest);

    // we're at crates/integration_tests.
    let workspace = manifest.join("../..");

    let golden_dir = workspace.join("crates/integration_tests/golden");
    let temp_artifacts_dir = workspace.join("target/integration_tests_artifacts");
    let assets_dir = workspace.join("crates/integration_tests/assets");

    for p in [&temp_artifacts_dir, &assets_dir, &golden_dir] {
        log::info!("Created environment directory {}", p.display());
        std::fs::create_dir_all(p).expect("Should be able to create test environment directory");
    }

    Environment {
        golden_dir,
        assets_dir,
        temp_artifacts_dir,
    }
}

pub fn get_env() -> Environment<&'static Path> {
    lazy_static::lazy_static! {
        static ref ENVIRONMENT: Environment<PathBuf> = compute_env();
    };

    Environment {
        golden_dir: &ENVIRONMENT.golden_dir,
        assets_dir: &ENVIRONMENT.assets_dir,
        temp_artifacts_dir: &ENVIRONMENT.temp_artifacts_dir,
    }
}

/// File to which test responses are written when the subprocess exits successfully.
pub const RESPONSE_GOOD_FILE: &str = "response.yml";

/// File to which a response is written if a test panics.
pub const RESPONSE_PANIC_FILE: &str = "response-panic.yml";

/// Environment variable which tells the test framework not to clear artifacts of passing tests.
pub const KEEP_ARTIFACTS_ENV_VAR: &str = "SYZ_TESTING_KEEP_ARTIFACTS";

impl<T: AsRef<Path>> Environment<T> {
    pub fn artifacts_dir_for(&self, test_name: &str) -> PathBuf {
        self.temp_artifacts_dir.as_ref().join(test_name)
    }

    pub fn panic_response_file_for(&self, test_name: &str) -> PathBuf {
        self.artifacts_dir_for(test_name).join(RESPONSE_PANIC_FILE)
    }

    pub fn good_response_file_for(&self, test_name: &str) -> PathBuf {
        self.artifacts_dir_for(test_name).join(RESPONSE_GOOD_FILE)
    }
}