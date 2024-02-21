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
pub const RESPONSE_GOOD_FILE: &str = "response.json";

/// File to which a response is written if a test panics.
pub const RESPONSE_PANIC_FILE: &str = "response-panic.json";
