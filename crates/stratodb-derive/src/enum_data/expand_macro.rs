use super::{accessors::accessors, load_arm::load_arm, store_arm::store_arm, variant_parts::VariantParts};
use crate::{attr::ContainerAttrs, desc::enum_desc};
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{DataEnum, DeriveInput, Error, Result as SynResult};

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

    // The catch-all `#[strato(other)]` variant: at most one, unit, never untagged.
    let other_variant = {
        let mut others = parts.iter().filter(|part| part.is_other());
        match (others.next(), others.next()) {
            (None, _) => None,
            (Some(_), Some(second)) => {
                return Err(Error::new(
                    second.ident().span(),
                    "at most one variant may be `#[strato(other)]`",
                ));
            }
            (Some(first), None) => {
                if repr.is_untagged() {
                    return Err(Error::new(
                        first.ident().span(),
                        "`#[strato(other)]` is not supported on untagged enums",
                    ));
                }
                if !first.is_unit() {
                    return Err(Error::new(
                        first.ident().span(),
                        "an `#[strato(other)]` variant must be a unit variant",
                    ));
                }

                Some(first)
            }
        }
    };

    let store_arms = parts.iter().map(|part| store_arm(part, &repr));
    let variant_names: Vec<String> = parts.iter().map(|part| part.tag().to_string()).collect();

    // Untagged has no tag to match on — each variant is tried in declaration order.
    let load_body = if repr.is_untagged() {
        let attempts = parts.iter().map(super::load_arm::untagged_arm);
        let error = container.no_match_error(quote! { ::std::format!("no untagged variant matched at '{at}'") });

        quote! {
            #(#attempts)*

            ::core::result::Result::Err(#error)
        }
    } else {
        let tag_load = repr.tag_load();
        // The `other` variant is the match's catch-all, so it gets no arm of its own.
        let load_arms = parts
            .iter()
            .filter(|part| !part.is_other())
            .map(|part| load_arm(part, &repr));

        let catch_all = match other_variant {
            Some(part) => {
                let id = part.ident();

                quote! { _ => ::core::result::Result::Ok(Self::#id), }
            }
            None => {
                let error = container.no_match_error(quote! { ::std::format!("unknown enum variant tag: {tag}") });

                quote! { _ => ::core::result::Result::Err(#error), }
            }
        };

        quote! {
            #tag_load

            match tag.as_str() {
                #(#load_arms)*
                #catch_all
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
