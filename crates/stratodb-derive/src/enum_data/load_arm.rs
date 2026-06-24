use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Fields, Variant};

/// A `match tag` arm that rebuilds one variant from its stored payload.
pub(super) fn load_arm(variant: &Variant) -> TokenStream2 {
    let id = &variant.ident;
    let tag = id.to_string();

    match &variant.fields {
        Fields::Unit => quote! {
            #tag => ::core::result::Result::Ok(Self::#id),
        },
        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
            let ty = &fields.unnamed[0].ty;

            quote! {
                #tag => ::core::result::Result::Ok(Self::#id(
                    <#ty as ::stratodb::data::SData>::load(reader, &at.child_name(#tag))?,
                )),
            }
        }
        Fields::Unnamed(fields) => {
            let loads = fields.unnamed.iter().enumerate().map(|(index, field)| {
                let ty = &field.ty;
                let index = index as u64;

                quote! { <#ty as ::stratodb::data::SData>::load(reader, &payload.child_index(#index))? }
            });

            quote! {
                #tag => {
                    let payload = at.child_name(#tag);
                    ::core::result::Result::Ok(Self::#id( #(#loads),* ))
                }
            }
        }
        Fields::Named(fields) => {
            let inits = fields.named.iter().map(|field| {
                let name = field.ident.as_ref().unwrap();
                let name_str = name.to_string();
                let ty = &field.ty;

                quote! { #name: <#ty as ::stratodb::data::SData>::load(reader, &payload.child_name(#name_str))? }
            });

            quote! {
                #tag => {
                    let payload = at.child_name(#tag);
                    ::core::result::Result::Ok(Self::#id { #(#inits),* })
                }
            }
        }
    }
}
