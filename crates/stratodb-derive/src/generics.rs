//! Propagation of a type's generics and bounds onto the generated `SData` impl
//! and accessor types.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{punctuated::Punctuated, GenericParam, Token, WherePredicate};

/// A parsed `bound = "..."` predicate list (e.g. `T: SData, U: Clone`).
pub(crate) type Bounds = Punctuated<WherePredicate, Token![,]>;

/// Pre-rendered generics fragments for the code generators. Built once per type
/// by [`Generics::analyze`]; for a non-generic type every fragment is empty (bar
/// the accessors' `'t`), so the generated code is unchanged.
pub(crate) struct Generics {
    sdata_impl:     TokenStream2,
    sdata_ty:       TokenStream2,
    sdata_where:    TokenStream2,
    accessor_impl:  TokenStream2,
    accessor_ty:    TokenStream2,
    accessor_where: TokenStream2,
    phantom_field:  TokenStream2,
    phantom_init:   TokenStream2,
}

impl Generics {
    /// Builds the fragments from a type's declared generics, applying either the
    /// custom `bound` predicates (which replace the synthesized bounds) or the
    /// default `T: SData` on every type parameter.
    pub(crate) fn analyze(generics: &syn::Generics, bound: Option<&Bounds>) -> Self {
        let mut sdata = generics.clone();
        {
            let where_clause = sdata.make_where_clause();
            match bound {
                Some(predicates) => where_clause.predicates.extend(predicates.iter().cloned()),
                None => {
                    for param in &generics.params {
                        if let GenericParam::Type(type_param) = param {
                            let ident = &type_param.ident;
                            where_clause
                                .predicates
                                .push(syn::parse_quote! { #ident: ::stratodb::data::SData });
                        }
                    }
                }
            }
        }

        // Accessors carry an extra `'t` for the borrowed reader/writer.
        let mut accessor = sdata.clone();
        accessor.params.insert(0, syn::parse_quote!('t));

        let (sdata_impl, sdata_ty, sdata_where) = sdata.split_for_impl();
        let (accessor_impl, accessor_ty, accessor_where) = accessor.split_for_impl();

        // Type parameters appear only in the accessors' getter return types, never
        // their fields, so a `PhantomData` keeps each one "used" (avoids E0392).
        let type_params: Vec<&syn::Ident> = generics.type_params().map(|param| &param.ident).collect();
        let (phantom_field, phantom_init) = if type_params.is_empty() {
            (quote! {}, quote! {})
        } else {
            (
                quote! { __marker: ::core::marker::PhantomData<fn() -> ( #(#type_params,)* )>, },
                quote! { __marker: ::core::marker::PhantomData, },
            )
        };

        Self {
            sdata_impl: quote! { #sdata_impl },
            sdata_ty: quote! { #sdata_ty },
            sdata_where: quote! { #sdata_where },
            accessor_impl: quote! { #accessor_impl },
            accessor_ty: quote! { #accessor_ty },
            accessor_where: quote! { #accessor_where },
            phantom_field,
            phantom_init,
        }
    }

    /// `impl #sdata_impl SData for Name #sdata_ty #sdata_where`.
    pub(crate) fn sdata_impl(&self) -> &TokenStream2 {
        &self.sdata_impl
    }

    pub(crate) fn sdata_ty(&self) -> &TokenStream2 {
        &self.sdata_ty
    }

    pub(crate) fn sdata_where(&self) -> &TokenStream2 {
        &self.sdata_where
    }

    /// Accessor header generics, including the extra `'t`.
    pub(crate) fn accessor_impl(&self) -> &TokenStream2 {
        &self.accessor_impl
    }

    pub(crate) fn accessor_ty(&self) -> &TokenStream2 {
        &self.accessor_ty
    }

    pub(crate) fn accessor_where(&self) -> &TokenStream2 {
        &self.accessor_where
    }

    /// A trailing `__marker: PhantomData<…>,` field (empty without type params).
    pub(crate) fn phantom_field(&self) -> &TokenStream2 {
        &self.phantom_field
    }

    /// The matching `__marker: PhantomData,` initializer (empty without type params).
    pub(crate) fn phantom_init(&self) -> &TokenStream2 {
        &self.phantom_init
    }
}
