use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{Fields, Ident, Variant};

/// A `match self` arm that writes one variant's payload under its tag.
pub(super) fn store_arm(variant: &Variant) -> TokenStream2 {
    let id = &variant.ident;
    let tag = id.to_string();

    match &variant.fields {
        Fields::Unit => quote! {
            Self::#id => {
                ::stratodb::access::Writer::put_scalar(writer, &at.child_name(#tag), ::stratodb::data::Scalar::Null)?;
            }
        },
        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => quote! {
            Self::#id(inner) => {
                ::stratodb::data::SData::store(inner, writer, &at.child_name(#tag))?;
            }
        },
        Fields::Unnamed(fields) => {
            let binds: Vec<Ident> = (0..fields.unnamed.len()).map(|i| format_ident!("f{}", i)).collect();
            let stores = binds.iter().enumerate().map(|(index, bind)| {
                let index = index as u64;

                quote! { ::stratodb::data::SData::store(#bind, writer, &payload.child_index(#index))?; }
            });

            quote! {
                Self::#id( #(#binds),* ) => {
                    let payload = at.child_name(#tag);
                    ::stratodb::access::Writer::ensure_container(writer, &payload, true)?;
                    #(#stores)*
                }
            }
        }
        Fields::Named(fields) => {
            let names: Vec<&Ident> = fields.named.iter().map(|f| f.ident.as_ref().unwrap()).collect();
            let name_strs: Vec<String> = names.iter().map(|n| n.to_string()).collect();

            quote! {
                Self::#id { #(#names),* } => {
                    let payload = at.child_name(#tag);
                    ::stratodb::access::Writer::ensure_container(writer, &payload, false)?;
                    #( ::stratodb::data::SData::store(#names, writer, &payload.child_name(#name_strs))?; )*
                }
            }
        }
    }
}
