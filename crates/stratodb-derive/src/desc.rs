//! The `StratoXxxDesc` companion type: lightweight, compile-time metadata about
//! a derived type's members (a struct's field names, or an enum's variant names).

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Ident, Visibility};

/// Generates `StratoXxxDesc` for a struct: its type name and field names.
pub(crate) fn struct_desc(vis: &Visibility, desc_name: &Ident, type_name: &str, fields: &[String]) -> TokenStream2 {
    quote! {
        #[allow(dead_code)]
        #vis struct #desc_name;

        #[allow(dead_code)]
        impl #desc_name {
            /// The Rust type name this descriptor describes.
            #vis const TYPE_NAME: &'static str = #type_name;

            /// The struct's field names, in declaration order.
            #vis const FIELDS: &'static [&'static str] = &[ #(#fields),* ];
        }
    }
}

/// Generates `StratoXxxDesc` for an enum: its type name and variant names.
pub(crate) fn enum_desc(vis: &Visibility, desc_name: &Ident, type_name: &str, variants: &[String]) -> TokenStream2 {
    quote! {
        #[allow(dead_code)]
        #vis struct #desc_name;

        #[allow(dead_code)]
        impl #desc_name {
            /// The Rust type name this descriptor describes.
            #vis const TYPE_NAME: &'static str = #type_name;

            /// The enum's variant names, in declaration order.
            #vis const VARIANTS: &'static [&'static str] = &[ #(#variants),* ];
        }
    }
}
