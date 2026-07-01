use crate::{field_parts::FieldParts, generics::Generics};
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Ident;

/// The write accessor type, its field getters, and its `SMut`/`SIdentifiable` impls.
pub(crate) fn mut_type(
    vis: &syn::Visibility,
    mut_name: &Ident,
    parts: &[FieldParts],
    generics: &Generics,
) -> TokenStream2 {
    let impl_generics = generics.accessor_impl();
    let ty_generics = generics.accessor_ty();
    let where_clause = generics.accessor_where();
    let phantom_field = generics.phantom_field();
    let phantom_init = generics.phantom_init();

    let getters = parts.iter().filter(|p| p.attrs().in_shape()).map(|p| {
        let setter = &p.setter();
        let ty = p.ty();
        let field = &p.name();

        // A flattened field shares the parent's node: open the accessor right there.
        if p.attrs().is_flatten() {
            return quote! {
                #vis fn #setter(&self) -> ::stratodb::SdbResult<<#ty as ::stratodb::data::SData>::Mut<'t>> {
                    ::core::result::Result::Ok(
                        <<#ty as ::stratodb::data::SData>::Mut<'t> as ::stratodb::data::refs::SMut<'t>>::open(
                            ::std::sync::Arc::clone(&self.writer),
                            self.base.clone(),
                            self.key,
                        ),
                    )
                }
            };
        }

        quote! {
            #vis fn #setter(&self) -> ::stratodb::SdbResult<<#ty as ::stratodb::data::SData>::Mut<'t>> {
                let at = self.base.child_name(#field);
                let key = ::stratodb::access::Reader::child_cached(
                    &self.writer,
                    self.key,
                    &::stratodb::path::Segment::Name(#field.into()),
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
        #vis struct #mut_name #impl_generics #where_clause {
            writer: ::std::sync::Arc<dyn ::stratodb::access::Writer + 't>,
            base:   ::stratodb::path::SPath,
            key:    ::stratodb::Skey,
            #phantom_field
        }

        #[allow(dead_code)]
        impl #impl_generics #mut_name #ty_generics #where_clause {
            #(#getters)*
        }

        #[automatically_derived]
        impl #impl_generics ::stratodb::data::refs::SMut<'t> for #mut_name #ty_generics #where_clause {
            fn open(
                writer: ::std::sync::Arc<dyn ::stratodb::access::Writer + 't>,
                base: ::stratodb::path::SPath,
                key: ::stratodb::Skey,
            ) -> Self {
                Self {
                    writer,
                    base,
                    key,
                    #phantom_init
                }
            }
        }

        #[automatically_derived]
        impl #impl_generics ::stratodb::data::refs::SIdentifiable for #mut_name #ty_generics #where_clause {
            fn key(&self) -> ::stratodb::Skey {
                self.key
            }

            fn path(&self) -> &::stratodb::path::SPath {
                &self.base
            }
        }
    }
}
