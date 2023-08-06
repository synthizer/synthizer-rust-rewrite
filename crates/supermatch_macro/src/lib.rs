use proc_macro::TokenStream;

use proc_macro_error::abort;
use syn::{parse_quote_spanned, spanned::Spanned};

fn optional_expr_to_numeric(expr: Option<&syn::Expr>) -> Option<(i64, &str)> {
    match expr? {
        syn::Expr::Lit(l_expr) => {
            if let syn::Lit::Int(l) = &l_expr.lit {
                match l.base10_parse::<i64>() {
                    Ok(x) => Some((x, l.suffix())),
                    Err(e) => abort!(l, "Unable to parse to i64: {}", e),
                }
            } else {
                None
            }
        }
        syn::Expr::Unary(u) => {
            if let syn::UnOp::Neg(_) = u.op {
                let (inner_num, inner_suffix) = optional_expr_to_numeric(Some(&u.expr))?;
                let num = match inner_num.checked_mul(-1) {
                    Some(x) => x,
                    None => abort!(expr, "Cannot negate {}", inner_num),
                };

                Some((num, inner_suffix))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn expand_pat(pattern: &syn::Pat) -> Vec<(syn::Pat, Option<syn::ItemConst>)> {
    use syn::Pat;

    match pattern {
        Pat::Ident(pi) => {
            let Some((_,maybe_range)) = pi.subpat.as_ref() else {
                return vec![(pattern.clone(),None)];
            };

            let Pat::Range(r) = &**maybe_range else {
                return vec![(pattern.clone(), None)];
            };

            // If we can get integers off both ends, then we proceed.
            let Some((lower_int, lower_suffix)) = optional_expr_to_numeric(r.start.as_deref()) else {
    return vec![(pattern.clone(), None)];
};

            let Some((upper_int, upper_suffix)) = optional_expr_to_numeric(r.end.as_deref()) else {return vec![(pattern.clone(),None)];};

            let suffix = if !lower_suffix.is_empty() {
                lower_suffix
            } else if !upper_suffix.is_empty() {
                upper_suffix
            } else {
                abort!(
                    r,
                    "A suffix on one of the integer literals is required. Try `0..5u64` or similar"
                );
            };

            // We may need to offset upper_int.
            let upper_offset: i64 = match r.limits {
                syn::RangeLimits::Closed(..) => -1,
                _ => 0,
            };

            let upper_int = match upper_int.checked_sub(upper_offset) {
                Some(x) => x,
                None => abort!(r.end, "This is already i64::MIN"),
            };

            if upper_int < lower_int {
                // Don't do anything; this is dead code. We can't really provide warnings at the current time due to
                // limitations in proc macros.
                return vec![(pattern.clone(), None)];
            }

            let suffix = syn::Ident::new(suffix, r.span());

            (lower_int..upper_int)
                .map(|i| {
                    let i = syn::LitInt::new(&i.to_string(), pi.span());

                    let pat = parse_quote_spanned!(pattern.span() => #i);
                    let ident = &pi.ident;

                    let decl = parse_quote_spanned!(
                        pi.span() =>
                        #[allow(non_upper_case_globals)]
                        const #ident: #suffix = #i;
                    );

                    (pat, Some(decl))
                })
                .collect::<Vec<_>>()
        }
        Pat::Or(branches) => branches
            .cases
            .iter()
            .cloned()
            .map(|p| (p, None))
            .collect::<Vec<_>>(),
        _ => vec![(pattern.clone(), None)],
    }
}

fn expand_match(what: &syn::ExprMatch) -> syn::ExprMatch {
    let new_arms = what
        .arms
        .iter()
        .flat_map(|arm| {
            let new_pats = expand_pat(&arm.pat);
            new_pats.into_iter().map(|(pat, decl)| {
                let old_body = &arm.body;
                let new_body = if decl.is_some() {
                    parse_quote_spanned!(old_body.span() => {#decl #old_body })
                } else {
                    old_body.clone()
                };
                syn::Arm {
                    pat,
                    body: new_body,
                    ..arm.clone()
                }
            })
        })
        .collect::<Vec<_>>();

    let mut out = syn::ExprMatch {
        arms: new_arms,
        ..what.clone()
    };

    out.attrs.retain(|a| !a.path().is_ident("supermatch"));
    out
}

/// Mark a function as able to contain enhanced match statements.
///
/// Once so marked it becomes possible to use [supermatch] in this function.  Failure to marka function will end up with
/// compile-time errors about unrecognized attributes, or errors about experimental attributes on expressions.
///
/// See the module-level documentation for more.
#[proc_macro_attribute]
pub fn supermatch_fn(_attribute: TokenStream, input: TokenStream) -> TokenStream {
    use syn::fold::Fold;

    struct SupermatchVisitor;

    impl Fold for SupermatchVisitor {
        fn fold_expr_match(&mut self, i: syn::ExprMatch) -> syn::ExprMatch {
            let nmatch = if i.attrs.iter().any(|a| a.path().is_ident("supermatch")) {
                expand_match(&i)
            } else {
                i
            };

            syn::fold::fold_expr_match(self, nmatch)
        }
    }

    let input: syn::ItemFn = syn::parse_macro_input!(input);
    let output = syn::fold::fold_item_fn(&mut SupermatchVisitor, input);
    quote::quote!(#output).into()
}
