//! Strato-paths: slash-separated addresses into the node tree.
//!
//! A path is a sequence of [`Segment`]s. Object fields are named (`a/b`); list
//! elements are indexed (`a/t[5]`). Indices bind to the preceding name without a
//! separator, so `a/t[5]/x` parses as `a`, `t`, `[5]`, `x`. A path is resolved by
//! walking the node tree (see [`crate::tree`]); it is never persisted, so it has
//! no byte encoding.

use crate::error::{SdbError, SdbResult};
use std::{fmt, str::FromStr};

/// One component of an [`SPath`].
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Segment {
    /// An object field name, e.g. `h` in `a/h`.
    Name(String),
    /// A zero-based list index, e.g. `5` in `a/t[5]`.
    Index(u64),
}

/// A parsed strato-path identifying a node in a table's tree.
#[derive(Clone, PartialEq, Eq, Hash, Default)]
pub struct SPath {
    segments: Vec<Segment>,
}

impl SPath {
    /// The root path (empty), identifying the whole table tree.
    pub fn root() -> Self {
        Self {
            segments: Vec::new()
        }
    }

    /// Parses a path string such as `a/b[12]/x`.
    pub fn parse(s: &str) -> SdbResult<Self> {
        s.parse()
    }

    /// Returns `true` if this is the root (empty) path.
    pub fn is_root(&self) -> bool {
        self.segments.is_empty()
    }

    /// The number of segments.
    pub fn len(&self) -> usize {
        self.segments.len()
    }

    /// Returns `true` if there are no segments (the root path).
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// The path's segments, in order.
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// The last segment, if any.
    pub fn last(&self) -> Option<&Segment> {
        self.segments.last()
    }

    /// Appends a named segment.
    pub fn push_name(&mut self, name: impl Into<String>) {
        self.segments.push(Segment::Name(name.into()));
    }

    /// Appends an indexed segment.
    pub fn push_index(&mut self, index: u64) {
        self.segments.push(Segment::Index(index));
    }

    /// Returns a copy of this path with a named segment appended.
    pub fn child_name(&self, name: impl Into<String>) -> Self {
        let mut path = self.clone();
        path.push_name(name);
        path
    }

    /// Returns a copy of this path with an indexed segment appended.
    pub fn child_index(&self, index: u64) -> Self {
        let mut path = self.clone();
        path.push_index(index);
        path
    }

    /// Returns this path followed by `tail`'s segments — `tail` resolved relative
    /// to `self`. Joining segment lists (rather than path strings) keeps index
    /// segments unambiguous, since they attach to a name without a separator.
    pub fn join(&self, tail: &SPath) -> Self {
        let mut segments = self.segments.clone();
        segments.extend_from_slice(&tail.segments);

        SPath {
            segments,
        }
    }

    /// The parent path, or `None` for the root.
    pub fn parent(&self) -> Option<SPath> {
        if self.segments.is_empty() {
            None
        } else {
            Some(SPath {
                segments: self.segments[..self.segments.len() - 1].to_vec(),
            })
        }
    }

    /// Splits into the parent path and the last segment, or `None` for the root.
    pub(crate) fn split_last(&self) -> Option<(SPath, &Segment)> {
        self.segments.split_last().map(|(last, head)| {
            (
                SPath {
                    segments: head.to_vec(),
                },
                last,
            )
        })
    }
}

fn validate_name(name: &str, full: &str) -> SdbResult<()> {
    if name.contains(['/', '[', ']']) {
        return Err(SdbError::InvalidPath(format!(
            "reserved character in segment '{name}' of '{full}'"
        )));
    }

    Ok(())
}

fn parse_token(token: &str, full: &str, out: &mut Vec<Segment>) -> SdbResult<()> {
    let Some(bracket) = token.find('[') else {
        validate_name(token, full)?;
        out.push(Segment::Name(token.to_string()));
        return Ok(());
    };

    let name = &token[..bracket];
    if !name.is_empty() {
        validate_name(name, full)?;
        out.push(Segment::Name(name.to_string()));
    }

    let mut rest = &token[bracket..];
    while !rest.is_empty() {
        if !rest.starts_with('[') {
            return Err(SdbError::InvalidPath(format!("expected '[' in segment of '{full}'")));
        }

        let close = rest
            .find(']')
            .ok_or_else(|| SdbError::InvalidPath(format!("unclosed '[' in '{full}'")))?;
        let digits = &rest[1..close];
        let index: u64 = digits
            .parse()
            .map_err(|_| SdbError::InvalidPath(format!("invalid index '{digits}' in '{full}'")))?;
        out.push(Segment::Index(index));

        rest = &rest[close + 1..];
    }

    Ok(())
}

impl FromStr for SPath {
    type Err = SdbError;

    fn from_str(s: &str) -> SdbResult<Self> {
        if s.is_empty() {
            return Ok(SPath::root());
        }

        let mut segments = Vec::new();
        for token in s.split('/') {
            if token.is_empty() {
                return Err(SdbError::InvalidPath(format!("empty segment in '{s}'")));
            }
            parse_token(token, s, &mut segments)?;
        }

        Ok(SPath {
            segments,
        })
    }
}

impl fmt::Display for SPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for segment in &self.segments {
            match segment {
                Segment::Name(name) => {
                    if !first {
                        f.write_str("/")?;
                    }
                    f.write_str(name)?;
                }
                Segment::Index(index) => write!(f, "[{index}]")?,
            }

            first = false;
        }

        Ok(())
    }
}

impl fmt::Debug for SPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SPath(\"{self}\")")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segs(path: &SPath) -> &[Segment] {
        path.segments()
    }

    #[test]
    fn parses_and_displays() {
        let p = SPath::parse("a/t[5]/x").unwrap();

        assert_eq!(
            segs(&p),
            &[
                Segment::Name("a".into()),
                Segment::Name("t".into()),
                Segment::Index(5),
                Segment::Name("x".into()),
            ]
        );

        assert_eq!(p.to_string(), "a/t[5]/x");
    }

    #[test]
    fn root_roundtrips() {
        let p = SPath::parse("").unwrap();

        assert!(p.is_root());
        assert_eq!(p.to_string(), "");
    }

    #[test]
    fn leading_index_and_nested() {
        let p = SPath::parse("[3]/a[0][1]").unwrap();

        assert_eq!(
            segs(&p),
            &[
                Segment::Index(3),
                Segment::Name("a".into()),
                Segment::Index(0),
                Segment::Index(1),
            ]
        );

        assert_eq!(p.to_string(), "[3]/a[0][1]");
    }

    #[test]
    fn rejects_empty_segments() {
        assert!(SPath::parse("a//b").is_err());
        assert!(SPath::parse("/a").is_err());
    }
}
