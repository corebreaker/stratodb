//! `#[derive(SData)]` for a type stored AS a target type `U`, via
//! `#[strato(into = "U", from = "U")]` / `#[strato(into = "U", try_from = "U")]`.
//!
//! Such a type is fully represented by `U` on disk: `store` converts to `U` with
//! `Into` then delegates, `load` reconstructs from `U` with `From`/`TryFrom`, and
//! the accessors ARE `U`'s (`type Ref/Mut = <U as SData>::Ref/Mut`) so a field of
//! this type inside another derived struct surfaces `U`'s accessor. No
//! `StratoXxx`/`StratoXxxDesc` is generated, so newtype/tuple structs and enums
//! are all accepted here — the field shape is never inspected.

use crate::attr::ContainerAttrs;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{DeriveInput, Error, Result as SynResult};

/// Generates the delegating `SData` impl for a `from`/`into`/`try_from` type.
pub(crate) fn convert_impl(input: &DeriveInput, container: &ContainerAttrs) -> SynResult<TokenStream2> {
    let name = &input.ident;

    // Index columns name a struct's fields; a delegated type exposes none.
    if let Some(index) = container.indexes().first() {
        return Err(Error::new(
            index.name.span(),
            "`#[strato(index(...))]` is incompatible with `from`/`into`/`try_from`",
        ));
    }

    // The on-disk form is the `into` target — required so the value can be stored.
    let Some(into_ty) = container.store_as() else {
        let probe = container.load_from().or(container.try_load_from()).expect("delegates");

        return Err(Error::new_spanned(
            probe,
            "`from`/`try_from` needs a matching `into` to store the value",
        ));
    };

    // The load source is exactly one of `from` / `try_from`.
    let load_body = match (container.load_from(), container.try_load_from()) {
        (Some(_), Some(try_ty)) => {
            return Err(Error::new_spanned(
                try_ty,
                "`from` and `try_from` are mutually exclusive",
            ));
        }

        (Some(from_ty), None) => quote! {
            let value: #from_ty = <#from_ty as ::stratodb::data::SData>::load(reader, at)?;

            ::core::result::Result::Ok(::core::convert::From::from(value))
        },

        (None, Some(try_ty)) => quote! {
            let value: #try_ty = <#try_ty as ::stratodb::data::SData>::load(reader, at)?;

            <Self as ::core::convert::TryFrom<#try_ty>>::try_from(value)
                .map_err(|e| ::stratodb::SdbError::Conversion(::std::string::ToString::to_string(&e)))
        },

        (None, None) => {
            return Err(Error::new_spanned(
                into_ty,
                "`into` needs a matching `from` or `try_from` to load the value",
            ));
        }
    };

    Ok(quote! {
        #[automatically_derived]
        impl ::stratodb::data::SData for #name {
            type Ref<'t> = <#into_ty as ::stratodb::data::SData>::Ref<'t>;
            type Mut<'t> = <#into_ty as ::stratodb::data::SData>::Mut<'t>;

            fn store<W: ::stratodb::access::Writer>(
                &self,
                writer: &W,
                at: &::stratodb::path::SPath,
            ) -> ::stratodb::SdbResult<()> {
                let target: #into_ty = ::core::convert::Into::into(::core::clone::Clone::clone(self));

                ::stratodb::data::SData::store(&target, writer, at)
            }

            fn load<R: ::stratodb::access::Reader>(
                reader: &R,
                at: &::stratodb::path::SPath,
            ) -> ::stratodb::SdbResult<Self> {
                #load_body
            }
        }
    })
}
