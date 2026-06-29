use proc_macro2::Span;
use syn::{parse::ParseStream, Ident, LitStr, Path, PathSegment, Result as SynResult, Type};

/// Parses a string literal holding a path: `"a::b"` parses to the path `a::b`.
pub(super) fn parse_path_lit(input: ParseStream) -> SynResult<Path> {
    input.parse::<LitStr>()?.parse()
}

/// Parses a string literal holding a type: `"Vec<u8>"` parses to the type `Vec<u8>`.
pub(super) fn parse_type_lit(input: ParseStream) -> SynResult<Type> {
    input.parse::<LitStr>()?.parse()
}

/// Returns `path::seg` — `path` with `seg` appended as a final segment.
pub(super) fn join_path(path: &Path, seg: &str) -> Path {
    let mut joined = path.clone();
    joined
        .segments
        .push(PathSegment::from(Ident::new(seg, Span::call_site())));
    joined
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
