use super::repr::EnumRepr;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Ident;

/// The minimal read/write accessors over an enum node: both expose `variant()`,
/// which reads the active tag the way the representation stored it.
pub(super) fn accessors(vis: &syn::Visibility, ref_name: &Ident, mut_name: &Ident, repr: &EnumRepr) -> TokenStream2 {
    let ref_variant = repr.variant_body(quote! { self.reader });
    let mut_variant = repr.variant_body(quote! { self.writer });

    quote! {
        #[allow(dead_code)]
        #vis struct #ref_name<'t> {
            reader: ::std::sync::Arc<dyn ::stratodb::access::Reader + 't>,
            base:   ::stratodb::path::SPath,
            key:    ::stratodb::Skey,
        }

        #[allow(dead_code)]
        impl<'t> #ref_name<'t> {
            /// The name of the currently-stored variant.
            #vis fn variant(&self) -> ::stratodb::SdbResult<::std::string::String> {
                #ref_variant
            }
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

        #[allow(dead_code)]
        #vis struct #mut_name<'t> {
            writer: ::std::sync::Arc<dyn ::stratodb::access::Writer + 't>,
            base:   ::stratodb::path::SPath,
            key:    ::stratodb::Skey,
        }

        #[allow(dead_code)]
        impl<'t> #mut_name<'t> {
            /// The name of the currently-stored variant.
            #vis fn variant(&self) -> ::stratodb::SdbResult<::std::string::String> {
                #mut_variant
            }
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
