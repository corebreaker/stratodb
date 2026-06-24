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
use syn::{parse::ParseStream, punctuated::Punctuated, Error, LitStr, Result as SynResult, Token};

/// A parsed `index(name = "...", columns(...), unique)` declaration.
pub(crate) struct IndexAttr {
    pub(crate) name:    LitStr,
    pub(crate) columns: Vec<ColumnSpec>,
    pub(crate) unique:  bool,
}

impl IndexAttr {
    /// Parses the body of an `index(...)` item — `input` is the parenthesized
    /// content (`name = "x", columns(a), unique`), the `index` keyword and parens
    /// already consumed by the container parser.
    pub(crate) fn from_body(input: ParseStream) -> SynResult<Self> {
        let span = input.span();
        let items = Punctuated::<Item, Token![,]>::parse_terminated(input)?;

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

        let name = name.ok_or_else(|| Error::new(span, "index requires `name = \"...\"`"))?;
        let columns = columns.ok_or_else(|| Error::new(span, "index requires `columns(...)`"))?;
        if columns.is_empty() {
            return Err(Error::new(span, "index requires at least one column"));
        }

        Ok(IndexAttr {
            name,
            columns,
            unique,
        })
    }
}
