use super::{repr::EnumRepr, variant_parts::VariantParts};
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Fields;

/// A `match tag` arm that rebuilds one variant from its stored payload. The arm
/// matches the primary tag and every alias; the payload is read from the
/// representation's base path.
pub(super) fn load_arm(parts: &VariantParts, repr: &EnumRepr) -> TokenStream2 {
    let id = parts.ident();
    let tag = parts.tag();
    let aliases = parts.aliases();
    let base = repr.payload_base_load();

    match parts.fields() {
        Fields::Unit => quote! {
            #tag #(| #aliases)* => ::core::result::Result::Ok(Self::#id),
        },
        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
            let ty = &fields.unnamed[0].ty;

            quote! {
                #tag #(| #aliases)* => ::core::result::Result::Ok(Self::#id(
                    <#ty as ::stratodb::data::SData>::load(reader, &#base)?,
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
                #tag #(| #aliases)* => {
                    let payload = #base;
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
                #tag #(| #aliases)* => {
                    let payload = #base;
                    ::core::result::Result::Ok(Self::#id { #(#inits),* })
                }
            }
        }
    }
}
