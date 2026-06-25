//! Generates the `SIndexed` impl from a struct's `#[strato(index(...))]` attrs.

use super::IndexAttr;
use crate::field_parts::FieldParts;

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Error, Ident, Result as SynResult};

/// Builds `impl SIndexed for #name` from the declared indexes, validating that
/// every column names a real field and resolving it to that field's *stored* node
/// name (so a `rename`d/`rename_all`d field indexes the right node). A struct with
/// no index attributes still gets an impl (returning no defs), so
/// [`Table::create_indexes`] works uniformly.
pub(crate) fn indexed_impl(name: &Ident, parts: &[FieldParts], indexes: &[IndexAttr]) -> SynResult<TokenStream2> {
    for index in indexes {
        for column in index.columns() {
            if !parts.iter().any(|part| part.getter() == column.field()) {
                return Err(Error::new(
                    column.field().span(),
                    format!("index column `{col}` is not a field of `{name}`", col = column.field()),
                ));
            }
        }
    }

    let defs = indexes.iter().map(|index| {
        let index_name = index.name();
        let unique = index.unique();
        let columns = index.columns().iter().map(|column| {
            let field = parts
                .iter()
                .find(|part| part.getter() == column.field())
                .expect("column validated above")
                .name();

            let path = quote! { ::stratodb::path::SPath::root().child_name(#field) };
            if column.descending() {
                quote! { ::stratodb::index::IndexColumn::desc(#path) }
            } else {
                quote! { ::stratodb::index::IndexColumn::asc(#path) }
            }
        });

        quote! {
            ::stratodb::index::IndexDef::new(
                ::std::string::String::from(#index_name),
                ::std::string::String::from(pattern),
                ::std::vec![ #(#columns),* ],
                #unique,
            )
        }
    });

    Ok(quote! {
        #[automatically_derived]
        impl ::stratodb::index::SIndexed for #name {
            fn index_defs(pattern: &str) -> ::std::vec::Vec<::stratodb::index::IndexDef> {
                ::std::vec![ #(#defs),* ]
            }
        }
    })
}
