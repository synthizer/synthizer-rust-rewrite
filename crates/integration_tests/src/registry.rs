use crate::context::TestContext;
use crate::test_config::TestConfig;

#[derive(Debug, derive_more::Display)]
#[display(fmt = "{} (registered at {file}:{line}:{column}", "self.name()")]
pub struct TestRegistryEntry {
    /// Run both in the parent and the child. Builds configuration, and should be idempotent.
    pub config_fn: fn() -> TestConfig,

    /// The test itself.
    pub run_fn: fn(&mut TestContext) -> anyhow::Result<()>,

    /// The name of the test before replacing `::`
    ///
    /// Rust isn't smart enough to let us do this in macros or anything.
    pub unsanitized_name: &'static str,

    /// The name with all `:` replaced.
    pub name: &'static once_cell::race::OnceBox<String>,
    pub file: &'static str,
    pub line: u32,
    pub column: u32,
}

inventory::collect!(TestRegistryEntry);

/// Return a  list of tests in alphabetical order by name.
///
/// On the first call, the list of tests is registered and cached.  Panics if any two tests have the same name.
pub fn get_tests() -> impl Iterator<Item = &'static TestRegistryEntry> {
    lazy_static::lazy_static! {
        static ref TEST_CACHE: Vec<&'static TestRegistryEntry> = build_test_cache();
    }

    TEST_CACHE.iter().copied()
}

fn build_test_cache() -> Vec<&'static TestRegistryEntry> {
    use itertools::Itertools;

    let iterator = inventory::iter::<TestRegistryEntry>.into_iter();
    let mut ret = iterator.collect::<Vec<&'static TestRegistryEntry>>();
    ret.sort_unstable_by_key(|x| (x.name(), x.file, x.line, x.column));

    let mut duplicate_examples: Vec<&'static TestRegistryEntry> = vec![];

    let groups = ret.iter().group_by(|x| x.name());
    for (_, items) in groups.into_iter() {
        let items_vec = items.copied().collect::<Vec<&'static TestRegistryEntry>>();
        if items_vec.len() > 1 {
            duplicate_examples.extend(items_vec);
        }
    }

    if !duplicate_examples.is_empty() {
        eprintln!("Found Duplicate test registrations. Cannot proceed.  The following tests are registered more than once:");
        for e in duplicate_examples {
            eprintln!("  {}", e);
        }

        eprintln!("Quitting due to the above duplicate tests.");
        std::process::exit(1);
    }

    ret
}

pub fn get_test_by_name(test_name: &str) -> Option<&'static TestRegistryEntry> {
    get_tests().find(|x| x.name() == test_name)
}
impl TestRegistryEntry {
    pub fn name(&self) -> &str {
        self.name
            .get_or_init(|| {
                Box::new(
                    self.unsanitized_name
                        .to_string()
                        .replace("::", ".")
                        .strip_prefix("synthizer_integration_tests.tests.")
                        .expect("All tests belong under the tests submodule")
                        .to_string(),
                )
            })
            .as_str()
    }
}
