use crate::{field_parts::FieldParts, generics::Generics};
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Ident;

/// The read accessor type, its field getters, and its `SRef`/`SIdentifiable` impls.
pub(crate) fn ref_type(
    vis: &syn::Visibility,
    ref_name: &Ident,
    parts: &[FieldParts],
    generics: &Generics,
) -> TokenStream2 {
    let impl_generics = generics.accessor_impl();
    let ty_generics = generics.accessor_ty();
    let where_clause = generics.accessor_where();
    let phantom_field = generics.phantom_field();
    let phantom_init = generics.phantom_init();

    let getters = parts.iter().filter(|p| p.attrs().in_shape()).map(|p| {
        let getter = p.getter();
        let ty = p.ty();
        let field = &p.name();

        // A flattened field shares the parent's node: open the accessor right there.
        if p.attrs().is_flatten() {
            return quote! {
                #vis fn #getter(&self) -> ::stratodb::SdbResult<<#ty as ::stratodb::data::SData>::Ref<'t>> {
                    ::core::result::Result::Ok(
                        <<#ty as ::stratodb::data::SData>::Ref<'t> as ::stratodb::data::refs::SRef<'t>>::open(
                            ::std::sync::Arc::clone(&self.reader),
                            self.base.clone(),
                            self.key,
                        ),
                    )
                }
            };
        }

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
        #vis struct #ref_name #impl_generics #where_clause {
            reader: ::std::sync::Arc<dyn ::stratodb::access::Reader + 't>,
            base:   ::stratodb::path::SPath,
            key:    ::stratodb::Skey,
            #phantom_field
        }

        #[allow(dead_code)]
        impl #impl_generics #ref_name #ty_generics #where_clause {
            #(#getters)*
        }

        #[automatically_derived]
        impl #impl_generics ::stratodb::data::refs::SRef<'t> for #ref_name #ty_generics #where_clause {
            fn open(
                reader: ::std::sync::Arc<dyn ::stratodb::access::Reader + 't>,
                base: ::stratodb::path::SPath,
                key: ::stratodb::Skey,
            ) -> Self {
                Self {
                    reader,
                    base,
                    key,
                    #phantom_init
                }
            }
        }

        #[automatically_derived]
        impl #impl_generics ::stratodb::data::refs::SIdentifiable for #ref_name #ty_generics #where_clause {
            fn key(&self) -> ::stratodb::Skey {
                self.key
            }

            fn path(&self) -> &::stratodb::path::SPath {
                &self.base
            }
        }
    }
}
