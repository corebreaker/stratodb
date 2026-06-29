//! Field-level `#[strato(...)]` attributes.

use super::{
    default::FieldDefault,
    misc::{join_path, parse_path_lit},
};

use syn::{parse::ParseStream, Attribute, Error, Ident, LitStr, Path, Result as SynResult, Token};
use proc_macro2::{Span, TokenStream as TokenStream2};
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
    /// Custom store function (`store_with = "path"`) replacing the field type's `SData::store`.
    store_with:    Option<Path>,
    /// Custom load function (`load_with = "path"`) replacing the field type's `SData::load`.
    load_with:     Option<Path>,
    /// Module supplying both `store` and `load` (`with = "module"`); sugar for the two above.
    with:          Option<Path>,
    /// Flatten the field's (object) value into the parent's node (stored/loaded at the parent's path).
    flatten:       Option<Span>,
}

impl FieldAttrs {
    pub(crate) fn parse(attrs: &[Attribute]) -> SynResult<Self> {
        let mut this = FieldAttrs::default();
        for attr in attrs.iter().filter(|attr| attr.path().is_ident("strato")) {
            attr.parse_args_with(|input: ParseStream| this.parse_items(input))?;
        }

        this.check_conflicts()?;
        Ok(this)
    }

    /// Rejects attribute combinations that cannot both hold.
    fn check_conflicts(&self) -> SynResult<()> {
        // A flattened field has no named node of its own, so no other attribute applies.
        if let Some(span) = self.flatten
            && (self.rename.is_some()
                || !self.aliases.is_empty()
                || self.skip
                || self.skip_store
                || self.skip_load
                || self.skip_store_if.is_some()
                || self.default.is_some()
                || self.with.is_some()
                || self.store_with.is_some()
                || self.load_with.is_some())
        {
            return Err(Error::new(
                span,
                "`flatten` cannot be combined with other field attributes",
            ));
        }

        // `with` already sets both sides; an explicit `store_with`/`load_with` is redundant.
        if self.with.is_some()
            && let Some(dup) = self.store_with.as_ref().or(self.load_with.as_ref())
        {
            return Err(Error::new_spanned(
                dup,
                "`store_with`/`load_with` cannot be combined with `with`",
            ));
        }

        // A custom store only runs for a stored field.
        if let Some(store) = self.store_with.as_ref().or(self.with.as_ref())
            && (self.skip || self.skip_store)
        {
            return Err(Error::new_spanned(
                store,
                "`store_with`/`with` conflicts with `skip`/`skip_store`: the field is never stored",
            ));
        }

        // A custom load only runs for a loaded field.
        if let Some(load) = self.load_with.as_ref().or(self.with.as_ref())
            && (self.skip || self.skip_load)
        {
            return Err(Error::new_spanned(
                load,
                "`load_with`/`with` conflicts with `skip`/`skip_load`: the field is never loaded",
            ));
        }

        Ok(())
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
                "store_with" => {
                    input.parse::<Token![=]>()?;
                    self.store_with = Some(parse_path_lit(input)?);
                }
                "load_with" => {
                    input.parse::<Token![=]>()?;
                    self.load_with = Some(parse_path_lit(input)?);
                }
                "with" => {
                    input.parse::<Token![=]>()?;
                    self.with = Some(parse_path_lit(input)?);
                }
                "flatten" => self.flatten = Some(key.span()),
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

    /// The function storing the field: an explicit `store_with`, else `with`'s
    /// `store`, else `None` to fall back to the field type's `SData::store`.
    pub(crate) fn store_fn(&self) -> Option<Path> {
        self.store_with
            .clone()
            .or_else(|| self.with.as_ref().map(|m| join_path(m, "store")))
    }

    /// The function loading the field: an explicit `load_with`, else `with`'s
    /// `load`, else `None` to fall back to the field type's `SData::load`.
    pub(crate) fn load_fn(&self) -> Option<Path> {
        self.load_with
            .clone()
            .or_else(|| self.with.as_ref().map(|m| join_path(m, "load")))
    }

    /// Whether the field is flattened into the parent's node (stored/loaded at the parent's path).
    pub(crate) fn is_flatten(&self) -> bool {
        self.flatten.is_some()
    }
}
