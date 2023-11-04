//! Implementation of derives for ToNamedInputs and ToNamedOutputs.
//!
//! These share a lot of code because they are identical trait bodies, just with different names for the functions.
use darling::FromDeriveInput;
use proc_macro_error::abort;

#[derive(FromDeriveInput)]
#[darling(supports(struct_named))]
struct NamedArgs {
    ident: syn::Ident,
    data: darling::ast::Data<(), syn::Field>,
}

/// Parameters for performing the generation of whichever case we are generating.
struct Opts {
    /// Full path to the trait.
    trait_path: syn::Path,

    /// Name of the function.
    func_name: syn::Ident,

    /// Name of the argument to the function.
    arg_name: syn::Ident,

    /// Type of the argument, pointing at nodes::traits::BYIndex.
    arg_ty_path: syn::Path,
}

/// Punch out either named inputs or outputs.
fn punchout(opts: Opts, tokens: proc_macro2::TokenStream) -> proc_macro2::TokenStream {
    let derive_input: syn::DeriveInput = match syn::parse2(tokens) {
        Ok(x) => x,
        Err(e) => {
            let span = e.span();
            abort!(span, e);
        }
    };

    let input = match NamedArgs::from_derive_input(&derive_input) {
        Ok(x) => x,
        Err(e) => {
            let span = e.span();
            abort!(span, e);
        }
    };

    let Opts {
        arg_name,
        arg_ty_path,
        func_name,
        trait_path,
    } = opts;

    let struct_ident = input.ident;

    // This is validated by darling already.
    let fields = input
        .data
        .take_struct()
        .unwrap()
        .fields
        .into_iter()
        .map(|x| x.ident.unwrap())
        .collect::<Vec<_>>();
    if fields.is_empty() {
        proc_macro_error::abort_call_site!("This struct must have at least one field. If you are trying to have no inputs or outputs, use `()` here instead.");
    }

    // We want to avoid continually shifting vectors forward. To do that, we will reverse the fields and pop from the
    // back of the vector.
    let mut fields_rev = fields;
    fields_rev.reverse();

    quote::quote!(
        impl<'a> #trait_path<'a> for #struct_ident<'a> {
            fn #func_name<'b>(#arg_name: &'b mut #arg_ty_path<'a>) -> #struct_ident<'a> {
                #struct_ident {
                    #(
                        #fields_rev: #arg_name
                            .pop()
                            .expect("Mismatch between the node descriptor and the number of fields in this struct")
                    ),*
                }
            }
        }
    )
}

fn ident_cs(name: &str) -> syn::Ident {
    syn::Ident::new(name, proc_macro2::Span::call_site())
}

fn path_cs(path: &str) -> syn::Path {
    use std::str::FromStr;
    syn::parse2(proc_macro2::TokenStream::from_str(path).unwrap()).unwrap()
}

pub(crate) fn derive_to_named_outputs_impl(
    tokens: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    punchout(
        Opts {
            arg_name: ident_cs("outputs"),
            arg_ty_path: path_cs("crate::nodes::OutputsByIndex"),
            func_name: ident_cs("to_named_outputs"),
            trait_path: path_cs("crate::nodes::traits::ToNamedOutputs"),
        },
        tokens.into(),
    )
    .into()
}

pub(crate) fn derive_to_named_inputs_impl(
    tokens: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    punchout(
        Opts {
            arg_name: ident_cs("inputs"),
            arg_ty_path: path_cs("crate::nodes::InputsByIndex"),
            func_name: ident_cs("to_named_inputs"),
            trait_path: path_cs("crate::nodes::traits::ToNamedInputs"),
        },
        tokens.into(),
    )
    .into()
}
