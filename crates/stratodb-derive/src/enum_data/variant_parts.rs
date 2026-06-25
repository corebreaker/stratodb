use crate::attr::{RenameRule, VariantAttrs};

use proc_macro2::Ident;
use syn::{Fields, Result as SynResult, Variant};

/// A single enum variant's parts: the variant itself plus its resolved stored
/// tag (`rename` > `rename_all` > the Rust name) and the tags accepted on load.
pub(super) struct VariantParts<'a> {
    variant: &'a Variant,
    tag:     String,
    aliases: Vec<String>,
    other:   bool,
}

impl<'a> VariantParts<'a> {
    pub(super) fn new(variant: &'a Variant, rename_all: Option<RenameRule>) -> SynResult<Self> {
        let attrs = VariantAttrs::parse(&variant.attrs)?;

        let tag = match attrs.rename() {
            Some(rename) => rename.to_string(),
            None => match rename_all {
                Some(rule) => rule.apply_to_variant(&variant.ident.to_string()),
                None => variant.ident.to_string(),
            },
        };

        Ok(Self {
            variant,
            tag,
            aliases: attrs.aliases().to_vec(),
            other: attrs.other(),
        })
    }

    pub(super) fn ident(&self) -> &Ident {
        &self.variant.ident
    }

    pub(super) fn fields(&self) -> &Fields {
        &self.variant.fields
    }

    pub(super) fn tag(&self) -> &str {
        &self.tag
    }

    pub(super) fn aliases(&self) -> &[String] {
        &self.aliases
    }

    /// Whether this is the `#[strato(other)]` catch-all variant.
    pub(super) fn is_other(&self) -> bool {
        self.other
    }

    /// Whether this is a unit variant (no payload).
    pub(super) fn is_unit(&self) -> bool {
        matches!(self.variant.fields, Fields::Unit)
    }
}
