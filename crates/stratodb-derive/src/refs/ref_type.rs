use crate::field_parts::FieldParts;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Ident;

/// The read accessor type, its field getters, and its `SRef`/`SIdentifiable` impls.
pub(crate) fn ref_type(vis: &syn::Visibility, ref_name: &Ident, parts: &[FieldParts]) -> TokenStream2 {
    let getters = parts.iter().map(|p| {
        let getter = p.getter();
        let ty = p.ty();
        let field = &p.name();

        quote! {
            #vis fn #getter(&self) -> ::stratodb::SdbResult<<#ty as ::stratodb::data::SData>::Ref<'t>> {
                let at = self.base.child_name(#field);
                let key = ::stratodb::access::Reader::child_cached(
                    &self.reader,
                    self.key,
                    &::stratodb::path::Segment::Name(::std::string::String::from(#field)),
                    &at,
                )?
                .ok_or_else(|| ::stratodb::SdbError::PathNotFound(at.clone()))?;

                ::core::result::Result::Ok(
                    <<#ty as ::stratodb::data::SData>::Ref<'t> as ::stratodb::data::refs::SRef<'t>>::open(
                        ::std::sync::Arc::clone(&self.reader),
                        at,
                        key,
                    ),
                )
            }
        }
    });

    quote! {
        #[allow(dead_code)]
        #vis struct #ref_name<'t> {
            reader: ::std::sync::Arc<dyn ::stratodb::access::Reader + 't>,
            base:   ::stratodb::path::SPath,
            key:    ::stratodb::Skey,
        }

        #[allow(dead_code)]
        impl<'t> #ref_name<'t> {
            #(#getters)*
        }

        #[automatically_derived]
        impl<'t> ::stratodb::data::refs::SRef<'t> for #ref_name<'t> {
            fn open(
                reader: ::std::sync::Arc<dyn ::stratodb::access::Reader + 't>,
                base: ::stratodb::path::SPath,
                key: ::stratodb::Skey,
            ) -> Self {
                Self {
                    reader,
                    base,
                    key,
                }
            }
        }

        #[automatically_derived]
        impl<'t> ::stratodb::data::refs::SIdentifiable for #ref_name<'t> {
            fn key(&self) -> ::stratodb::Skey {
                self.key
            }

            fn path(&self) -> &::stratodb::path::SPath {
                &self.base
            }
        }
    }
}
