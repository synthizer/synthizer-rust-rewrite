/// Given an identifier an some code, wrap the code in a "sealed" module:
///
/// ```IGNORE
/// mod xxx_sealed {
///     use super::*;
///     tokens here...
/// }
///
/// pub(crate) use xxx_sealed::*;
/// ```
///
/// The sealed module name is derived from the identifier by converting it to snake case and adding _sealed.
pub(crate) fn seal<T: quote::ToTokens>(ident: &syn::Ident, tokens: &T) -> proc_macro2::TokenStream {
    use convert_case::Casing;

    let cased = ident.to_string().to_case(convert_case::Case::Snake);
    let mod_token = syn::Ident::new(&cased, ident.span());
    quote::quote!(
        mod #mod_token {
            use super::*;

            #tokens
        }

        pub(crate) use #mod_token::*;
    )
}

/// Builds a match statement like this:
///
/// ```IGNORE
/// match index {
///     0 => expr0,
/// 1 => expr1,
/// ...
/// _ => panic!("Index out of bounds"),
/// }
/// ```
///
/// This is used to allow turning user-supplied indices into references to fields on heterogeneous types, primarily
/// method calls, where we can't use arrays without overhead. This shows up in e.g. property slots, where propertis of
/// different types have different concrete representations.
pub(crate) fn build_indexing_match<I: Iterator<Item = impl quote::ToTokens>>(
    index: syn::Expr,
    items: I,
) -> syn::ExprMatch {
    let arms = items
        .enumerate()
        .map(|(index, expr)| quote::quote!(#index => #expr))
        .collect::<Vec<_>>();
    let arm_count = arms.len();
    let index_error = if arm_count > 0 {
        format!("Index {{}} must not be over {}", arm_count - 1)
    } else {
        "Got index {}, but no indices are accepted here; this is expected to be dead code"
            .to_string()
    };

    syn::parse_quote!(match #index {
        #(#arms),*,
        _ => panic!(#index_error, #index),
    })
}
