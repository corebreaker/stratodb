//! The `rename_all` casing rule.

use super::misc::{capitalize, lower_first};
use syn::{Error, LitStr, Result as SynResult};

/// A `rename_all` rule, named exactly as in Serde.
#[derive(Clone, Copy)]
pub(crate) enum RenameRule {
    Lower,
    Upper,
    Pascal,
    Camel,
    Snake,
    ScreamingSnake,
    Kebab,
    ScreamingKebab,
}

impl RenameRule {
    pub(crate) fn from_lit(lit: &LitStr) -> SynResult<Self> {
        let rule = match lit.value().as_str() {
            "lowercase" => Self::Lower,
            "UPPERCASE" => Self::Upper,
            "PascalCase" => Self::Pascal,
            "camelCase" => Self::Camel,
            "snake_case" => Self::Snake,
            "SCREAMING_SNAKE_CASE" => Self::ScreamingSnake,
            "kebab-case" => Self::Kebab,
            "SCREAMING-KEBAB-CASE" => Self::ScreamingKebab,
            other => {
                return Err(Error::new(
                    lit.span(),
                    format!(
                        "unknown rename rule `{other}`; expected one of lowercase, UPPERCASE, \
                         PascalCase, camelCase, snake_case, SCREAMING_SNAKE_CASE, kebab-case, \
                         SCREAMING-KEBAB-CASE"
                    ),
                ));
            }
        };

        Ok(rule)
    }

    /// Applies the rule to a struct field name (a Rust field is `snake_case`).
    pub(crate) fn apply_to_field(self, field: &str) -> String {
        match self {
            Self::Lower | Self::Snake => field.to_owned(),
            Self::Upper | Self::ScreamingSnake => field.to_ascii_uppercase(),
            Self::Pascal => field.split('_').map(capitalize).collect(),
            Self::Camel => lower_first(&field.split('_').map(capitalize).collect::<String>()),
            Self::Kebab => field.replace('_', "-"),
            Self::ScreamingKebab => field.to_ascii_uppercase().replace('_', "-"),
        }
    }

    /// Applies the rule to an enum variant name (a Rust variant is `PascalCase`).
    pub(crate) fn apply_to_variant(self, variant: &str) -> String {
        match self {
            Self::Pascal => variant.to_owned(),
            Self::Lower => variant.to_ascii_lowercase(),
            Self::Upper => variant.to_ascii_uppercase(),
            Self::Camel => lower_first(variant),
            Self::Snake => split_pascal(variant, '_', false),
            Self::ScreamingSnake => split_pascal(variant, '_', true),
            Self::Kebab => split_pascal(variant, '-', false),
            Self::ScreamingKebab => split_pascal(variant, '-', true),
        }
    }
}

/// Lower- or upper-cases `variant`, inserting `sep` before each interior capital
/// (`FooBar` -> `foo_bar` / `FOO-BAR`).
fn split_pascal(variant: &str, sep: char, screaming: bool) -> String {
    let mut out = String::new();
    for (i, ch) in variant.chars().enumerate() {
        if i > 0 && ch.is_ascii_uppercase() {
            out.push(sep);
        }

        out.push(if screaming {
            ch.to_ascii_uppercase()
        } else {
            ch.to_ascii_lowercase()
        });
    }

    out
}
