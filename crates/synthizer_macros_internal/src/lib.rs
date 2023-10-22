//! This crate contains macros for Synthizer's internal use.
//!
//! Synthizer has a lot of internal boilerplate by virtue of needing to be realtime-safe, for example declaring commands
//! which will go cross-thread for every possible function which might need to run on the audio thread.  It also has a
//! lot of boilerplate abstraction, for example understanding the concept of a property and building node descriptors.
//! Rather than type this tens of times and refactor them all tens of times per change, we have this crate to help us
//! out.
//!
//! This crate contains dummy forwarders at the root to let us split it into files. See the individual modules for docs
//! on the macro. Procmacro limitations currently require that they be at the root.
mod property_slots_impl;
mod utils;

#[proc_macro_attribute]
#[proc_macro_error::proc_macro_error]
pub fn property_slots(
    attrs: proc_macro::TokenStream,
    body: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    property_slots_impl::property_slots_impl(attrs.into(), body.into()).into()
}
