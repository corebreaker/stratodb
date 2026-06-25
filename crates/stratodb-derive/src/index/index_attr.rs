//! Parsing for the `#[strato(index(...))]` attribute.
//!
//! Each occurrence on a struct declares one index:
//!
//! ```ignore
//! #[strato(index(name = "by_age_name", columns(age, name desc), unique))]
//! ```
//!
//! `name` and a non-empty `columns(...)` list are required; `unique` is an
//! optional flag. A column is a field name with an optional `asc` (default) or
//! `desc` direction; columns are listed in priority order.

use super::{item::Item, ColumnSpec};
use syn::{
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    Attribute,
    Error,
    Ident,
    LitStr,
    Token,
    Result as SynResult,
};

/// A parsed `index(name = "...", columns(...), unique)` declaration.
pub(crate) struct IndexAttr {
    pub(crate) name:    LitStr,
    pub(crate) columns: Vec<ColumnSpec>,
    pub(crate) unique:  bool,
}

impl Parse for IndexAttr {
    fn parse(input: ParseStream) -> SynResult<Self> {
        // `input` is the body of `strato(...)`, e.g. `index(name = "x", columns(a), unique)`.
        let keyword = input.parse::<Ident>()?;
        if keyword != "index" {
            return Err(Error::new(
                keyword.span(),
                "unsupported `#[strato(...)]` attribute; expected `index(...)`",
            ));
        }

        let inner;
        syn::parenthesized!(inner in input);
        let items = Punctuated::<Item, Token![,]>::parse_terminated(&inner)?;

        let mut name = None;
        let mut columns = None;
        let mut unique = false;
        for item in items {
            match item {
                Item::Name(value) => name = Some(value),
                Item::Columns(value) => columns = Some(value),
                Item::Unique => unique = true,
            }
        }

        let name = name.ok_or_else(|| Error::new(keyword.span(), "index requires `name = \"...\"`"))?;
        let columns = columns.ok_or_else(|| Error::new(keyword.span(), "index requires `columns(...)`"))?;
        if columns.is_empty() {
            return Err(Error::new(keyword.span(), "index requires at least one column"));
        }

        Ok(IndexAttr {
            name,
            columns,
            unique,
        })
    }
}

/// Parses every `#[strato(index(...))]` declaration from `attrs`.
pub(crate) fn index_attrs(attrs: &[Attribute]) -> SynResult<Vec<IndexAttr>> {
    attrs
        .iter()
        .filter(|attr| attr.path().is_ident("strato"))
        .map(|attr| attr.parse_args::<IndexAttr>())
        .collect()
}
