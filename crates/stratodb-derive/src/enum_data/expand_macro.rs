use super::{accessors::accessors, load_arm::load_arm, store_arm::store_arm, variant_parts::VariantParts};
use crate::{attr::ContainerAttrs, desc::enum_desc};
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{DataEnum, DeriveInput, Result as SynResult};

pub(crate) fn expand_enum(input: &DeriveInput, data: &DataEnum, container: &ContainerAttrs) -> SynResult<TokenStream2> {
    let vis = &input.vis;
    let name = &input.ident;
    let ref_name = format_ident!("Strato{}", name);
    let mut_name = format_ident!("Strato{}Mut", name);
    let desc_name = format_ident!("Strato{}Desc", name);

    let repr = super::repr::EnumRepr::from_container(container, name)?;

    let parts = data
        .variants
        .iter()
        .map(|variant| VariantParts::new(variant, container.rename_all()))
        .collect::<SynResult<Vec<_>>>()?;

    let store_arms = parts.iter().map(|part| store_arm(part, &repr));
    let variant_names: Vec<String> = parts.iter().map(|part| part.tag().to_string()).collect();

    // Untagged has no tag to match on — each variant is tried in declaration order.
    let load_body = if repr.is_untagged() {
        let attempts = parts.iter().map(super::load_arm::untagged_arm);

        quote! {
            #(#attempts)*

            ::core::result::Result::Err(::stratodb::SdbError::Corrupt(::std::format!(
                "no untagged variant matched at '{at}'"
            )))
        }
    } else {
        let tag_load = repr.tag_load();
        let load_arms = parts.iter().map(|part| load_arm(part, &repr));

        quote! {
            #tag_load

            match tag.as_str() {
                #(#load_arms)*
                other => ::core::result::Result::Err(::stratodb::SdbError::Corrupt(::std::format!(
                    "unknown enum variant tag: {other}"
                ))),
            }
        }
    };

    let sdata_impl = quote! {
        #[automatically_derived]
        impl ::stratodb::data::SData for #name {
            type Ref<'t> = #ref_name<'t>;
            type Mut<'t> = #mut_name<'t>;

            fn store<W: ::stratodb::access::Writer>(
                &self,
                writer: &W,
                at: &::stratodb::path::SPath,
            ) -> ::stratodb::SdbResult<()> {
                // The node carries exactly one variant, so clear any prior one first.
                ::stratodb::access::Writer::remove(writer, at)?;

                match self {
                    #(#store_arms)*
                }

                ::core::result::Result::Ok(())
            }

            fn load<R: ::stratodb::access::Reader>(
                reader: &R,
                at: &::stratodb::path::SPath,
            ) -> ::stratodb::SdbResult<Self> {
                #load_body
            }
        }
    };

    let accessors = accessors(vis, &ref_name, &mut_name, &repr);
    let desc = enum_desc(vis, &desc_name, &name.to_string(), &variant_names);

    Ok(quote! {
        #sdata_impl

        #accessors

        #desc
    })
}
