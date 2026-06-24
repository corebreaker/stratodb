//! Field-level `#[strato(...)]` attributes.

use super::{default::FieldDefault, misc::parse_path_lit};
use syn::{parse::ParseStream, Attribute, Error, Ident, LitStr, Path, Result as SynResult, Token};
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;

/// The `#[strato(...)]` attributes that apply to a single field.
#[derive(Default)]
pub(crate) struct FieldAttrs {
    /// Overrides the field's stored node name.
    rename:        Option<String>,
    /// Extra node names accepted when loading (the primary name is still used to store).
    aliases:       Vec<String>,
    /// Never store or load this field (load uses [`default`](Self::default)).
    skip:          bool,
    /// Never store this field.
    skip_store:    bool,
    /// Never load this field (load uses [`default`](Self::default)).
    skip_load:     bool,
    /// Skip storing when this predicate (`fn(&Field) -> bool`) returns true.
    skip_store_if: Option<Path>,
    /// How to produce the value on load when the node is absent or skipped.
    default:       Option<FieldDefault>,
}

impl FieldAttrs {
    pub(crate) fn parse(attrs: &[Attribute]) -> SynResult<Self> {
        let mut this = FieldAttrs::default();
        for attr in attrs.iter().filter(|attr| attr.path().is_ident("strato")) {
            attr.parse_args_with(|input: ParseStream| this.parse_items(input))?;
        }

        Ok(this)
    }

    /// Whether the field is part of the stored shape — it drives the accessor
    /// getter and `Desc::FIELDS`. A never-stored field (`skip`/`skip_store`) is not.
    pub(crate) fn in_shape(&self) -> bool {
        !self.skip && !self.skip_store
    }

    /// Whether `load` reads the field from its stored node. A field that is never
    /// stored (`skip`/`skip_store`) or never loaded (`skip_load`) takes its
    /// [`default`](Self::default_expr) instead.
    pub(crate) fn loads_from_node(&self) -> bool {
        self.in_shape() && !self.skip_load
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
                "skip" => self.skip = true,
                "skip_store" => self.skip_store = true,
                "skip_load" => self.skip_load = true,
                "skip_store_if" => {
                    input.parse::<Token![=]>()?;
                    self.skip_store_if = Some(parse_path_lit(input)?);
                }
                "default" => {
                    self.default = Some(if input.peek(Token![=]) {
                        input.parse::<Token![=]>()?;
                        FieldDefault::Path(parse_path_lit(input)?)
                    } else {
                        FieldDefault::Trait
                    });
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

    /// The default value expression: `path()` for `default = "path"`, else
    /// `Default::default()` (for `default`, `skip`, or `skip_load`).
    pub(crate) fn default_expr(&self) -> TokenStream2 {
        match &self.default {
            Some(FieldDefault::Path(path)) => quote! { #path() },
            _ => quote! { ::core::default::Default::default() },
        }
    }

    pub(crate) fn rename(&self) -> Option<&str> {
        self.rename.as_deref()
    }

    pub(crate) fn aliases(&self) -> &[String] {
        &self.aliases
    }

    pub(crate) fn skip_store_if(&self) -> Option<&Path> {
        self.skip_store_if.as_ref()
    }

    pub(crate) fn field_default(&self) -> Option<&FieldDefault> {
        self.default.as_ref()
    }
}
