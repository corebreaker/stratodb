use crate::field_parts::FieldParts;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Ident;

/// The write accessor type, its field getters, and its `SMut`/`SIdentifiable` impls.
pub(crate) fn mut_type(vis: &syn::Visibility, mut_name: &Ident, parts: &[FieldParts]) -> TokenStream2 {
    let getters = parts.iter().filter(|p| p.attrs().in_shape()).map(|p| {
        let setter = &p.setter();
        let ty = p.ty();
        let field = &p.name();

        quote! {
            #vis fn #setter(&self) -> ::stratodb::SdbResult<<#ty as ::stratodb::data::SData>::Mut<'t>> {
                let at = self.base.child_name(#field);
                let key = ::stratodb::access::Reader::child_cached(
                    &self.writer,
                    self.key,
                    &::stratodb::path::Segment::Name(::std::string::String::from(#field)),
                    &at,
                )?
                .ok_or_else(|| ::stratodb::SdbError::PathNotFound(at.clone()))?;

                ::core::result::Result::Ok(
                    <<#ty as ::stratodb::data::SData>::Mut<'t> as ::stratodb::data::refs::SMut<'t>>::open(
                        ::std::sync::Arc::clone(&self.writer),
                        at,
                        key,
                    ),
                )
            }
        }
    });

    quote! {
        #[allow(dead_code)]
        #vis struct #mut_name<'t> {
            writer: ::std::sync::Arc<dyn ::stratodb::access::Writer + 't>,
            base:   ::stratodb::path::SPath,
            key:    ::stratodb::Skey,
        }

        #[allow(dead_code)]
        impl<'t> #mut_name<'t> {
            #(#getters)*
        }

        #[automatically_derived]
        impl<'t> ::stratodb::data::refs::SMut<'t> for #mut_name<'t> {
            fn open(
                writer: ::std::sync::Arc<dyn ::stratodb::access::Writer + 't>,
                base: ::stratodb::path::SPath,
                key: ::stratodb::Skey,
            ) -> Self {
                Self {
                    writer,
                    base,
                    key,
                }
            }
        }

        #[automatically_derived]
        impl<'t> ::stratodb::data::refs::SIdentifiable for #mut_name<'t> {
            fn key(&self) -> ::stratodb::Skey {
                self.key
            }

            fn path(&self) -> &::stratodb::path::SPath {
                &self.base
            }
        }
    }
}
