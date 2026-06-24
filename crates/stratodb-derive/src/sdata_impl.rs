use crate::field_parts::FieldParts;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Ident;

/// The `SData` impl: store/load shred the struct into one child node per field.
pub(crate) fn sdata_impl(name: &Ident, ref_name: &Ident, mut_name: &Ident, parts: &[FieldParts]) -> TokenStream2 {
    let store_fields = parts.iter().map(|p| {
        let getter = p.getter();
        let field = &p.name();

        quote! {
            ::stratodb::data::SData::store(&self.#getter, writer, &at.child_name(#field))?;
        }
    });

    let load_fields = parts.iter().map(|p| {
        let getter = p.getter();
        let ty = p.ty();
        let field = &p.name();

        quote! {
            #getter: <#ty as ::stratodb::data::SData>::load(reader, &at.child_name(#field))?,
        }
    });

    quote! {
        #[automatically_derived]
        impl ::stratodb::data::SData for #name {
            type Ref<'t> = #ref_name<'t>;
            type Mut<'t> = #mut_name<'t>;

            fn store<W: ::stratodb::access::Writer>(
                &self,
                writer: &W,
                at: &::stratodb::path::SPath,
            ) -> ::stratodb::SdbResult<()> {
                #(#store_fields)*

                ::core::result::Result::Ok(())
            }

            fn load<R: ::stratodb::access::Reader>(
                reader: &R,
                at: &::stratodb::path::SPath,
            ) -> ::stratodb::SdbResult<Self> {
                ::core::result::Result::Ok(Self {
                    #(#load_fields)*
                })
            }
        }
    }
}
