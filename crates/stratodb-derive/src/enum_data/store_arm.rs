use super::{repr::EnumRepr, variant_parts::VariantParts};
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{Fields, Ident};

/// A `match self` arm that writes one variant's tag and payload per the
/// representation. The tag goes to the object key (external) or a named field
/// (adjacent); the payload lands under the representation's base path.
pub(super) fn store_arm(parts: &VariantParts, repr: &EnumRepr) -> TokenStream2 {
    let id = parts.ident();
    let tag = parts.tag();
    let tag_store = repr.tag_store(tag);
    let base = repr.payload_base_store(tag);

    match parts.fields() {
        Fields::Unit => {
            let unit_body = repr.unit_store(tag);

            quote! {
                Self::#id => {
                    #unit_body
                }
            }
        }
        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => quote! {
            Self::#id(inner) => {
                #tag_store
                ::stratodb::data::SData::store(inner, writer, &#base)?;
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
                    #tag_store
                    let payload = #base;
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
                    #tag_store
                    let payload = #base;
                    ::stratodb::access::Writer::ensure_container(writer, &payload, false)?;
                    #( ::stratodb::data::SData::store(#names, writer, &payload.child_name(#name_strs))?; )*
                }
            }
        }
    }
}
