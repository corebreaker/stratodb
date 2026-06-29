//! Conversion into an [`SPath`] for path-addressed APIs.

use super::SPath;
use crate::error::SdbResult;

/// A value accepted where a path is expected: an owned or borrowed [`SPath`]
/// (used as-is) or a string (parsed, with the same normalization as
/// [`SPath::parse`]).
///
/// Path-addressed methods take `impl IntoPath`, so a literal like `"users/alice"`
/// and an already-built [`SPath`] are both accepted without a separate overload.
/// Parsing is fallible — a malformed string surfaces as an
/// [`InvalidPath`](crate::SdbError::InvalidPath) error — while an [`SPath`] passes
/// through unparsed.
pub trait IntoPath {
    /// Converts `self` into an [`SPath`], parsing it if it is a string.
    fn into_path(self) -> SdbResult<SPath>;
}

impl IntoPath for SPath {
    fn into_path(self) -> SdbResult<SPath> {
        Ok(self)
    }
}

impl IntoPath for &SPath {
    fn into_path(self) -> SdbResult<SPath> {
        Ok(self.clone())
    }
}

impl IntoPath for &str {
    fn into_path(self) -> SdbResult<SPath> {
        self.parse()
    }
}

impl IntoPath for String {
    fn into_path(self) -> SdbResult<SPath> {
        self.parse()
    }
}

impl IntoPath for &String {
    fn into_path(self) -> SdbResult<SPath> {
        self.parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_forms_parse() {
        let want = SPath::parse("a/b[2]/c").unwrap();

        assert_eq!("a/b[2]/c".into_path().unwrap(), want);
        assert_eq!(String::from("a/b[2]/c").into_path().unwrap(), want);
        assert_eq!((&String::from("a/b[2]/c")).into_path().unwrap(), want);
    }

    #[test]
    fn spath_forms_pass_through() {
        let p = SPath::parse("x/y").unwrap();

        assert_eq!(p.clone().into_path().unwrap(), p);
        assert_eq!((&p).into_path().unwrap(), p);
    }

    #[test]
    fn invalid_string_errors() {
        assert!("a//b".into_path().is_err());
    }
}
