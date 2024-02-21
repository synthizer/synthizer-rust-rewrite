use regex::bytes as regex;

use crate::cli_args::FilterArgs;
use crate::registry::TestRegistryEntry;

/// Given a string representing a glob pattern, return a regular xpression which can match it.
///
/// Since this is a CLI utility, panics on error.
fn compile_glob(glob: &str) -> regex::Regex {
    let parsed_glob = globset::Glob::new(glob).unwrap();
    let r = parsed_glob.regex();
    regex::Regex::new(r).unwrap()
}

/// Get an iterator over tests which match a filter from the command line.
pub fn get_tests_filtered(args: &FilterArgs) -> impl Iterator<Item = &'static TestRegistryEntry> {
    let maybe_glob = args.pattern.as_ref().map(|x| compile_glob(x.as_str()));

    crate::registry::get_tests().filter(move |x| {
        maybe_glob
            .as_ref()
            .map(|g| g.is_match(x.name().as_bytes()))
            .unwrap_or(true)
    })
}
