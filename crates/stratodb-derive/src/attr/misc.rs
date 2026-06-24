use syn::{parse::ParseStream, LitStr, Path, Result as SynResult};

/// Parses a string literal holding a path: `"a::b"` parses to the path `a::b`.
pub(super) fn parse_path_lit(input: ParseStream) -> SynResult<Path> {
    input.parse::<LitStr>()?.parse()
}

/// Upper-cases the first character of `word`, leaving the rest unchanged.
pub(super) fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// Lower-cases the first character of `s`, leaving the rest unchanged.
pub(super) fn lower_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_ascii_lowercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}
