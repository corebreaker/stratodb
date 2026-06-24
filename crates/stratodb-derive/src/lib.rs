//! Procedural macros for StratoDB.
//!
//! Provides `#[derive(SData)]`, which generates, for a struct, the two lazy
//! accessor types (`StratoXxx` read / `StratoXxxMut` write) plus the
//! [`SData`](../stratodb/data/trait.SData.html) implementation that shreds the
//! struct into one child node per field.
//!
//! The generated getters mirror the hand-written reference in
//! `stratodb/tests/typed.rs`: every field `f: F` yields a read getter `f()`
//! returning `<F as SData>::Ref` and a write getter `f_mut()` returning
//! `<F as SData>::Mut`, so scalar fields surface as `Leaf`/`LeafMut` and
//! composite fields as the nested accessor — uniformly, since the macro cannot
//! tell scalars from composites by type alone.
//!
//! v1 supports structs with named fields only. Enums, tuple/unit structs,
//! generics and `#[sdata(...)]` attributes are reported as errors for now.

mod expand_macro;
mod field_parts;
mod named_fields;
mod refs;
mod sdata_impl;

/// Derives [`SData`] for a struct with named fields.
#[proc_macro_derive(SData)]
pub fn derive_sdata(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);

    expand_macro::expand_macro(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
