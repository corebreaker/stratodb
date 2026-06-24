use syn::Path;

/// How an absent or skipped field is produced when loading.
pub(crate) enum FieldDefault {
    /// `#[strato(default)]` — `Default::default()`.
    Trait,
    /// `#[strato(default = "path")]` — `path()`.
    Path(Path),
}
