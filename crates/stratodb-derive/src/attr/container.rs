//! Type-level `#[strato(...)]` attributes.

use super::{misc::parse_type_lit, rename::RenameRule};
use crate::index::IndexAttr;

use syn::{parse::ParseStream, Attribute, Error, Ident, LitStr, Result as SynResult, Token, Type};

/// The `#[strato(...)]` attributes that apply to a whole type.
#[derive(Default)]
pub(crate) struct ContainerAttrs {
    rename_all: Option<RenameRule>,
    indexes:    Vec<IndexAttr>,
    from:       Option<Type>,
    into:       Option<Type>,
    try_from:   Option<Type>,
}

impl ContainerAttrs {
    pub(crate) fn parse(attrs: &[Attribute]) -> SynResult<Self> {
        let mut this = ContainerAttrs::default();
        for attr in attrs.iter().filter(|attr| attr.path().is_ident("strato")) {
            attr.parse_args_with(|input: ParseStream| this.parse_items(input))?;
        }

        Ok(this)
    }

    fn parse_items(&mut self, input: ParseStream) -> SynResult<()> {
        while !input.is_empty() {
            let key = input.parse::<Ident>()?;
            match key.to_string().as_str() {
                "rename_all" => {
                    input.parse::<Token![=]>()?;
                    self.rename_all = Some(RenameRule::from_lit(&input.parse::<LitStr>()?)?);
                }
                "index" => {
                    let body;
                    syn::parenthesized!(body in input);
                    self.indexes.push(IndexAttr::from_body(&body)?);
                }
                "from" => {
                    input.parse::<Token![=]>()?;
                    self.from = Some(parse_type_lit(input)?);
                }
                "into" => {
                    input.parse::<Token![=]>()?;
                    self.into = Some(parse_type_lit(input)?);
                }
                "try_from" => {
                    input.parse::<Token![=]>()?;
                    self.try_from = Some(parse_type_lit(input)?);
                }
                other => {
                    return Err(Error::new(
                        key.span(),
                        format!("unknown `strato` container attribute `{other}`"),
                    ));
                }
            }

            if input.is_empty() {
                break;
            }

            input.parse::<Token![,]>()?;
        }

        Ok(())
    }

    pub(crate) fn rename_all(&self) -> Option<RenameRule> {
        self.rename_all
    }

    pub(crate) fn indexes(&self) -> &[IndexAttr] {
        &self.indexes
    }

    /// The `from` target: the type `load` reconstructs from (infallibly).
    pub(crate) fn load_from(&self) -> Option<&Type> {
        self.from.as_ref()
    }

    /// The `into` target: the type the value is stored as.
    pub(crate) fn store_as(&self) -> Option<&Type> {
        self.into.as_ref()
    }

    /// The `try_from` target: the type `load` reconstructs from (fallibly).
    pub(crate) fn try_load_from(&self) -> Option<&Type> {
        self.try_from.as_ref()
    }

    /// Whether any of `from`/`into`/`try_from` makes this a delegated (stored-as-`U`) type.
    pub(crate) fn delegates(&self) -> bool {
        self.from.is_some() || self.into.is_some() || self.try_from.is_some()
    }
}
