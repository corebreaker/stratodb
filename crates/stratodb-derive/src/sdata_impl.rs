use crate::{field_parts::FieldParts, generics::Generics};
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Ident;

/// The `SData` impl: store/load shred the struct into one child node per field.
pub(crate) fn sdata_impl(
    name: &Ident,
    ref_name: &Ident,
    mut_name: &Ident,
    parts: &[FieldParts],
    generics: &Generics,
) -> TokenStream2 {
    let impl_generics = generics.sdata_impl();
    let ty_generics = generics.sdata_ty();
    let where_clause = generics.sdata_where();
    let accessor_ty = generics.accessor_ty();

    // Only stored-shape fields are written; `skip_store_if` makes it conditional.
    let store_fields = parts.iter().filter(|p| p.attrs().in_shape()).map(|p| {
        let getter = p.getter();
        let field = p.name();
        let store = match p.attrs().store_fn() {
            Some(path) => quote! { #path(&self.#getter, writer, &at.child_name(#field))?; },
            None => quote! { ::stratodb::data::SData::store(&self.#getter, writer, &at.child_name(#field))?; },
        };

        match p.attrs().skip_store_if() {
            Some(predicate) => quote! { if !#predicate(&self.#getter) { #store } },
            None => store,
        }
    });

    let load_fields = parts.iter().map(|p| {
        let getter = p.getter();
        let value = load_value(p);

        quote! { #getter: #value, }
    });

    quote! {
        #[automatically_derived]
        impl #impl_generics ::stratodb::data::SData for #name #ty_generics #where_clause {
            type Ref<'t> = #ref_name #accessor_ty;
            type Mut<'t> = #mut_name #accessor_ty;

            fn store<__W: ::stratodb::access::Writer>(
                &self,
                writer: &__W,
                at: &::stratodb::path::SPath,
            ) -> ::stratodb::SdbResult<()> {
                #(#store_fields)*

                ::core::result::Result::Ok(())
            }

            fn load<__R: ::stratodb::access::Reader>(
                reader: &__R,
                at: &::stratodb::path::SPath,
            ) -> ::stratodb::SdbResult<Self> {
                ::core::result::Result::Ok(Self {
                    #(#load_fields)*
                })
            }
        }
    }
}

/// The expression that produces a field's value on load.
fn load_value(p: &FieldParts) -> TokenStream2 {
    let ty = p.ty();
    let field = p.name();
    let attrs = p.attrs();

    // Never read (`skip`/`skip_store`/`skip_load`): use the default.
    if !attrs.loads_from_node() {
        return attrs.default_expr();
    }

    let load_fn = attrs.load_fn();
    let load_from = |name: TokenStream2| match &load_fn {
        Some(path) => quote! { #path(reader, &#name)? },
        None => quote! { <#ty as ::stratodb::data::SData>::load(reader, &#name)? },
    };

    // A direct read suffices unless we must look under aliases or fall back to a
    // default when the node is absent.
    if attrs.aliases().is_empty() && attrs.field_default().is_none() {
        return load_from(quote! { at.child_name(#field) });
    }

    let aliases = attrs.aliases();
    let fallback = match &attrs.field_default() {
        Some(_) => attrs.default_expr(),
        None => load_from(quote! { at.child_name(#field) }),
    };

    let load_chosen = load_from(quote! { at.child_name(candidate) });

    // Pick the primary name, else the first alias present; otherwise fall back.
    quote! {
        {
            let mut chosen: ::core::option::Option<&str> = ::core::option::Option::None;
            for candidate in [#field, #(#aliases),*] {
                if ::stratodb::access::Reader::resolve(reader, &at.child_name(candidate))?.is_some() {
                    chosen = ::core::option::Option::Some(candidate);
                    break;
                }
            }

            match chosen {
                ::core::option::Option::Some(candidate) => #load_chosen,
                ::core::option::Option::None => #fallback,
            }
        }
    }
}
