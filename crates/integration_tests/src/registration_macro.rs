/// Registers a test.
///
/// Takes a test name, which must correspond to a function in the invoking module with the signature `fn x(&mut
/// TestContext) -> anyhow::Result<()>`.  This will be called in the test's subprocess.  The module must also define `
/// fn test_name_config() -> TestConfig`, which returns test configuration.  The configuration function will be called
/// multiple times across different processes and threads.
macro_rules! register_test {
    ($test_fn:ident) => {
        inventory::submit! {
            crate::registry::TestRegistryEntry {
                config_fn: paste::paste!{ [<$test_fn _config>] },
                run_fn: $test_fn,
                unsanitized_name: concat!(module_path!(), ".", stringify!($test_fn)),
                name: {
                    static NAME_CACHE: once_cell::race::OnceBox::<String> = once_cell::race::OnceBox::new();
                    &NAME_CACHE
                },
                file: file!(),
                line: line!(),
                column: column!(),
            }
        }
    };
}
