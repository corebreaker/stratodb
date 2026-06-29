use super::repr::EnumRepr;
use crate::generics::Generics;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Ident;

/// The minimal read/write accessors over an enum node: both expose `variant()`,
/// which reads the active tag the way the representation stored it.
pub(super) fn accessors(
    vis: &syn::Visibility,
    ref_name: &Ident,
    mut_name: &Ident,
    repr: &EnumRepr,
    generics: &Generics,
) -> TokenStream2 {
    let impl_generics = generics.accessor_impl();
    let ty_generics = generics.accessor_ty();
    let where_clause = generics.accessor_where();
    let phantom_field = generics.phantom_field();
    let phantom_init = generics.phantom_init();

    let ref_variant = repr.variant_body(quote! { self.reader });
    let mut_variant = repr.variant_body(quote! { self.writer });

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
            /// The name of the currently-stored variant.
            #vis fn variant(&self) -> ::stratodb::SdbResult<::std::string::String> {
                #ref_variant
            }
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

        #[allow(dead_code)]
        #vis struct #mut_name #impl_generics #where_clause {
            writer: ::std::sync::Arc<dyn ::stratodb::access::Writer + 't>,
            base:   ::stratodb::path::SPath,
            key:    ::stratodb::Skey,
            #phantom_field
        }

        #[allow(dead_code)]
        impl #impl_generics #mut_name #ty_generics #where_clause {
            /// The name of the currently-stored variant.
            #vis fn variant(&self) -> ::stratodb::SdbResult<::std::string::String> {
                #mut_variant
            }
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
