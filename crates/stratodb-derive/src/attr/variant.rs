//! Enum-variant-level `#[strato(...)]` attributes.

use syn::{parse::ParseStream, Attribute, Error, Ident, LitStr, Result as SynResult, Token};

/// The `#[strato(...)]` attributes that apply to a single enum variant.
#[derive(Default)]
pub(crate) struct VariantAttrs {
    /// Overrides the variant's stored tag.
    rename:  Option<String>,
    /// Extra tags accepted when loading (the primary tag is still used to store).
    aliases: Vec<String>,
}

impl VariantAttrs {
    pub(crate) fn parse(attrs: &[Attribute]) -> SynResult<Self> {
        let mut this = VariantAttrs::default();
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
                        format!("unknown `strato` variant attribute `{other}`"),
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

    pub(crate) fn rename(&self) -> Option<&str> {
        self.rename.as_deref()
    }

    pub(crate) fn aliases(&self) -> &[String] {
        &self.aliases
    }
}
