use super::{accessors::accessors, load_arm::load_arm, store_arm::store_arm, variant_parts::VariantParts};
use crate::{attr::RenameRule, desc::enum_desc};
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{DataEnum, DeriveInput, Result as SynResult};

pub(crate) fn expand_enum(
    input: &DeriveInput,
    data: &DataEnum,
    rename_all: Option<RenameRule>,
) -> SynResult<TokenStream2> {
    let vis = &input.vis;
    let name = &input.ident;
    let ref_name = format_ident!("Strato{}", name);
    let mut_name = format_ident!("Strato{}Mut", name);
    let desc_name = format_ident!("Strato{}Desc", name);

    let parts = data
        .variants
        .iter()
        .map(|variant| VariantParts::new(variant, rename_all))
        .collect::<SynResult<Vec<_>>>()?;

    let store_arms = parts.iter().map(store_arm);
    let load_arms = parts.iter().map(load_arm);
    let variant_names: Vec<String> = parts.iter().map(|p| p.tag().to_string()).collect();

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
                // Externally tagged: the node carries exactly one key (the active
                // variant), so clear any previously-stored variant first.
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
                let key = ::stratodb::access::Reader::resolve(reader, at)?
                    .ok_or_else(|| ::stratodb::SdbError::PathNotFound(at.clone()))?;

                let tag = ::stratodb::access::Reader::object_keys(reader, key)?
                    .into_iter()
                    .next()
                    .ok_or_else(|| {
                        ::stratodb::SdbError::Corrupt(::std::string::String::from("enum node has no variant tag"))
                    })?;

                match tag.as_str() {
                    #(#load_arms)*
                    other => ::core::result::Result::Err(::stratodb::SdbError::Corrupt(::std::format!(
                        "unknown enum variant tag: {other}"
                    ))),
                }
            }
        }
    };

    let accessors = accessors(vis, &ref_name, &mut_name);
    let desc = enum_desc(vis, &desc_name, &name.to_string(), &variant_names);

    Ok(quote! {
        #sdata_impl

        #accessors

        #desc
    })
}
