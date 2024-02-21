use crate::cli_args::{CliArgs, ListArgs};

pub fn list(_top_args: &CliArgs, list_args: &ListArgs) {
    for i in crate::test_filtering::get_tests_filtered(&list_args.filter) {
        println!("{i}");
    }
}
