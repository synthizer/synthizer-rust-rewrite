//! A macro to handle PropertyCommandReceiver, which we apply to XXXSlots structs. This does two things:
//!
//! - Implements PropertyCommandReceiver.
//! - Creates a XXXProperties struct which is for the public API of a node.
//!
//! Input structs *must* be named with the Slots suffix, and will be sealed magically with this macro.  See the structs
//! in this file for input options.
//!
//! This is an attribute macro and not a derive because it must seal the struct, and so must have the ability to move
//! the original definition.  All struct-level options will go in the `property_slots` attribute (also the macro
//! invocation) and (later, when we implement options for such), field options will be `slot`.
//!
//! To document properties, document them on the input struct.  The docs are then copied over to the output and (in
//! future) augmented with additional metadata such as ranges.
//!
//! It is strictly assumed that all fields of a slots struct are `PropertySlot<Marker>`.
use darling::FromDeriveInput;
use syn::spanned::Spanned;

// Note that we can get a syn DeriveInput from an attribute macro simply by ignoring the attribute token stream.

#[derive(Debug, FromDeriveInput)]
#[darling(supports(struct_named))]
struct PropSlotsInput {
    ident: syn::Ident,
    data: darling::ast::Data<(), syn::Field>,
}

struct ParsedSlots {
    /// XXXSlots.
    slots_name: syn::Ident,

    /// XxxProperties.
    public_name: syn::Ident,
    slots: Vec<SlotMeta>,
}

struct SlotMeta {
    name: syn::Ident,
    marker: syn::Type,
}

fn parse_slots(input: PropSlotsInput) -> ParsedSlots {
    if !input.ident.to_string().ends_with("Slots") {
        proc_macro_error::abort!(input.ident, "Slot struct names must end with 'Slots'");
    }

    let slots_name = input.ident.clone();
    let mut public_name = input
        .ident
        .to_string()
        .strip_suffix("Slots")
        .expect("We validated that it ends with slots earlier")
        .to_string();
    public_name.push_str("Props");
    let public_name = syn::Ident::new(&public_name, proc_macro2::Span::call_site());

    let mut slots: Vec<SlotMeta> = vec![];

    for f in input
        .data
        .clone()
        .take_struct()
        .expect("Darling should validate this is a struct")
        .into_iter()
    {
        let name = f.ident.unwrap();
        let ty_path = match f.ty {
            syn::Type::Path(ref p) => p.clone(),
            _ => proc_macro_error::abort_call_site!(
                "Found a non-struct field in this struct. Fields should always be 'Slot<Marker>'"
            ),
        };

        let last_seg = match ty_path.path.segments.last() {
            Some(x) => x,
            None => {
                proc_macro_error::abort!(
                    f.ty.span(),
                    "This path has no segments; it should be of the form 'Slot<MarkerHere>'"
                );
            }
        };

        // That last segment should have exactly one generic; that generic should be the path to a concrete marker type.
        let args = match last_seg.arguments {
            syn::PathArguments::AngleBracketed(ref a) => a.clone(),
            _ => proc_macro_error::abort!(
                last_seg.span(),
                "This should have some generics on it, but does not"
            ),
        };

        let args = args.args;

        if args.len() != 1 {
            proc_macro_error::abort!(args.span(), "This should have exactly one generic argument");
        }

        let marker = match args[0] {
            syn::GenericArgument::Type(ref t) => t.clone(),
            _ => proc_macro_error::abort!(args[0].span(), "Thios should be a type"),
        };

        slots.push(SlotMeta { name, marker });
    }

    ParsedSlots {
        slots,
        slots_name,
        public_name,
    }
}

pub(crate) fn property_slots_impl(
    _attrs: proc_macro2::TokenStream,
    body: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let syn_input: syn::DeriveInput = match syn::parse2(body.clone()) {
        Ok(x) => x,
        Err(e) => proc_macro_error::abort_call_site!("{} while parsing DeriveInput", e),
    };

    let input = match PropSlotsInput::from_derive_input(&syn_input) {
        Ok(x) => x,
        Err(e) => {
            return e.write_errors();
        }
    };

    // The input to this macro is already a complete slots struct; sep 1 is to seal it.
    //
    // In future, we must remove attributes with a syn visitor, but at the moment there are none; alternatively we may
    // eventually opt to rewrite it from the field metadata.
    let sealed_slots = crate::utils::seal(&input.ident, &body);

    let parsed_meta = parse_slots(input);

    // Step 2 is to generate a struct for the public API, of the form XxxProperties.  We do this in three steps:
    //
    // 1. Figure out the identifier;
    // 2. Figure out the declaration;
    // 3. Figure out the PropertyCommandReceiver implementation.

    // Our generic lifetime is `'a`.
    let public_field_decls = parsed_meta
        .slots
        .iter()
        .map(|f| {
            let name = &f.name;
            let marker = &f.marker;
            quote::quote!(pub(crate) #name: crate::properties::Property<'a, #marker>)
        })
        .collect::<Vec<proc_macro2::TokenStream>>();

    let public_name = &parsed_meta.public_name;

    let pub_struct_decl = quote::quote!(pub struct #public_name<'a> {
        #(#public_field_decls),*
    });

    // The PropertyCommandReceiver trait has a few methods which are either matches based off indices or simple field expansions.  The field expansions can be done by name, so:
    let field_names = parsed_meta
        .slots
        .iter()
        .map(|x| &x.name)
        .collect::<Vec<_>>();

    // But the match ones need to be built up by hand so they can be passed through our utility function for such.

    let set_value_arms = parsed_meta.slots.iter().map(|x| {
        let name = &x.name;
        quote::quote_spanned!(name.span() => self.#name.set_from_property_value(value, crate::properties::ChangeState::Other))
    });

    let set_value_body =
        crate::utils::build_indexing_match(syn::parse_quote!(index), set_value_arms);

    let slots_name = &parsed_meta.slots_name;

    let property_command_receiver = quote::quote!(
        impl crate::properties::PropertyCommandReceiver for #slots_name {
            fn tick_first(&mut self) {
                #(self.#field_names.mark_first_tick());*
            }

            fn tick_ended(&mut self) {
                #(self.#field_names.mark_unchanged());*
            }

            fn set_property(&mut self, index: usize, value: PropertyValue) {
                #set_value_body;
            }
        }
    );

    // The last thing to do is to impl a new method for the public struct.
    let public_new_field_exprs = parsed_meta.slots.iter().enumerate().map(|(i, x)| {
        let name = &x.name;
        quote::quote!(#name: crate::properties::Property::new(sender, port, #i))
    });

    let public_impl_prop_getters = parsed_meta.slots.iter().map(|x| {
        let name = &x.name;
        let marker = &x.marker;

        quote::quote!(
            pub fn #name(&self) -> &crate::properties::Property<#marker> {
                &self.#name
            }
        )
    });

    let public_impls = quote::quote!(
        impl<'a> #public_name<'a> {
            /// Called by the .props() method on a handle.
            fn new(sender: &'a dyn crate::command::CommandSender, port: crate::command::Port) -> #public_name<'a> {
                #public_name {
                    #(#public_new_field_exprs),*
                }
            }

            #(#public_impl_prop_getters)*
        }
    );

    quote::quote!(
        #sealed_slots
        #pub_struct_decl
        #property_command_receiver
        #public_impls
    )
}
