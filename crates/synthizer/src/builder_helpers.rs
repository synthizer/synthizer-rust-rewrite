/// A macro to help validate required fields on a builder.
///
/// For example, `validate_required_fields!(builder, a, b, c)` will either declare and set variables `a`, `b`, and `c`,
/// or return `Err("Missing field a".into())` (so builders must consequently have error types which can be converted
/// from `&'static str`).
macro_rules! validate_required_fields {
    ($builder: expr, $($field: ident),*) => {
        $(
            let Some($field) = $builder.$field else {
                return Err(concat!("Missing field ", stringify!($field)).into());
            };
        )*
    }
}
