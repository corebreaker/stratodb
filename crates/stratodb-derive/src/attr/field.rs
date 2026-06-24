//! Field-level `#[strato(...)]` attributes.

use super::rename::RenameRule;
use crate::field_parts::FieldParts;
use syn::{parse::ParseStream, Attribute, Error, Ident, LitStr, Result as SynResult, Token, Type};
use quote::format_ident;

/// The `#[strato(...)]` attributes that apply to a single field.
#[derive(Default)]
pub(crate) struct FieldAttrs {
    /// Overrides the field's stored node name.
    rename:  Option<String>,
    /// Extra node names accepted when loading (the primary name is still used to store).
    aliases: Vec<String>,
}

impl FieldAttrs {
    pub(crate) fn parse(attrs: &[Attribute]) -> SynResult<Self> {
        let mut this = FieldAttrs::default();
        for attr in attrs.iter().filter(|attr| attr.path().is_ident("strato")) {
            attr.parse_args_with(|input: ParseStream| this.parse_items(input))?;
        }

        Ok(this)
    }

    fn parse_items(&mut self, input: ParseStream) -> SynResult<()> {
        while !input.is_empty() {
            let key = input.parse::<Ident>()?;
            match key.to_string().as_str() {
                "rename" => {
                    input.parse::<Token![=]>()?;
                    self.rename = Some(input.parse::<LitStr>()?.value());
                }
                "alias" => {
                    input.parse::<Token![=]>()?;
                    self.aliases.push(input.parse::<LitStr>()?.value());
                }
                other => {
                    return Err(Error::new(
                        key.span(),
                        format!("unknown `strato` field attribute `{other}`"),
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

    pub(crate) fn into_field_parts<'a>(
        self,
        ty: &'a Type,
        getter: &'a Ident,
        rename_all: Option<RenameRule>,
    ) -> FieldParts<'a> {
        let setter = format_ident!("{}_mut", getter);

        // Stored name: an explicit `rename` wins, else `rename_all`, else the field.
        let name = self.rename.unwrap_or_else(|| match rename_all {
            Some(rule) => rule.apply_to_field(&getter.to_string()),
            None => getter.to_string(),
        });

        FieldParts::new(
            getter,
            setter,
            ty,
            name,
            self.aliases,
        )
    }
}
