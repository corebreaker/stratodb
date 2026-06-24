//! The `rename_all` casing rule.

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
}

/// Upper-cases the first character of `word`, leaving the rest unchanged.
fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// Lower-cases the first character of `s`, leaving the rest unchanged.
fn lower_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_ascii_lowercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}
