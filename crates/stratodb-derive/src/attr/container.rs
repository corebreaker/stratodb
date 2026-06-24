//! Type-level `#[strato(...)]` attributes.

use super::rename::RenameRule;
use crate::index::IndexAttr;

use syn::{parse::ParseStream, Attribute, Error, Ident, LitStr, Result as SynResult, Token};

/// The `#[strato(...)]` attributes that apply to a whole type.
#[derive(Default)]
pub(crate) struct ContainerAttrs {
    rename_all: Option<RenameRule>,
    indexes:    Vec<IndexAttr>,
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
}
