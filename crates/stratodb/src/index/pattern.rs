//! Index path patterns: which entities an index covers.
//!
//! A pattern is a slash-separated path in which a `*` segment is a wildcard
//! matching any single child (`users/*` selects every direct child of `users`).
//! Non-wildcard segments are ordinary [`Segment`]s, so `a/t[0]` is allowed too.
//!
//! The pattern is also how a write decides what to re-index. Given the path a
//! mutation touched, [`Pattern::affected_entities`] returns exactly the matching
//! entities that lie on that path's root-to-node line — the only ones whose
//! indexed columns the mutation could have changed. An entity above the mutation
//! (its subtree changed) or at/below it (it was created, replaced or removed) is
//! affected; an entity on a different branch is not, and is never visited.

use crate::{
    engine::{TableKey, TableValue},
    error::{SdbError, SdbResult},
    path::{SPath, Segment},
    tree,
    Skey,
};

use redb::ReadableTable;

/// A parsed index pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pattern {
    segs: Vec<PatternSeg>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PatternSeg {
    /// A fixed segment that must match exactly.
    Lit(Segment),
    /// `*`: matches any single child.
    Star,
}

impl Pattern {
    /// Parses a pattern such as `users/*` or `org/teams/*`.
    pub(crate) fn parse(pattern: &str) -> SdbResult<Pattern> {
        let mut segs = Vec::new();
        if pattern.is_empty() {
            return Ok(Pattern {
                segs,
            });
        }

        for token in pattern.split('/') {
            if token.is_empty() {
                return Err(SdbError::InvalidPath(format!(
                    "empty segment in index pattern '{pattern}'"
                )));
            }

            if token == "*" {
                segs.push(PatternSeg::Star);
            } else {
                // Reuse path parsing for a single token (handles `name` and `name[i]`).
                for seg in SPath::parse(token)?.segments() {
                    segs.push(PatternSeg::Lit(seg.clone()));
                }
            }
        }

        Ok(Pattern {
            segs,
        })
    }

    /// Returns the keys of the entities this pattern matches that lie on the same
    /// root-to-node line as `scope` (the path a mutation touched). The walk is
    /// pruned to that line, so it touches only nodes the mutation could affect.
    pub(crate) fn affected_entities<T>(&self, t: &T, scope: &SPath) -> SdbResult<Vec<Skey>>
    where
        T: ReadableTable<TableKey, TableValue>, {
        let mut out = Vec::new();
        self.walk(t, 0, Skey::ROOT, scope.segments(), 0, &mut out)?;

        Ok(out)
    }

    fn walk<T>(
        &self,
        t: &T,
        si: usize,
        cur: Skey,
        scope: &[Segment],
        depth: usize,
        out: &mut Vec<Skey>,
    ) -> SdbResult<()>
    where
        T: ReadableTable<TableKey, TableValue>, {
        // Every pattern segment consumed: `cur` is a matched entity.
        if si == self.segs.len() {
            out.push(cur);
            return Ok(());
        }

        match &self.segs[si] {
            PatternSeg::Lit(seg) => {
                // Within `scope`, this segment must equal the path the mutation
                // took; otherwise the entity is on a different branch.
                if depth < scope.len() && &scope[depth] != seg {
                    return Ok(());
                }

                if let Some(child) = tree::child_key(t, cur, seg)? {
                    self.walk(t, si + 1, child, scope, depth + 1, out)?;
                }
            }
            PatternSeg::Star => {
                if depth < scope.len() {
                    // The wildcard binds to the one child the mutation descended into.
                    if let Some(child) = tree::child_key(t, cur, &scope[depth])? {
                        self.walk(t, si + 1, child, scope, depth + 1, out)?;
                    }
                } else {
                    // Past the mutation's path: it sits above these entities, so
                    // every child is in scope.
                    for child in tree::children(t, cur)? {
                        self.walk(t, si + 1, child, scope, depth + 1, out)?;
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit_name(name: &str) -> PatternSeg {
        PatternSeg::Lit(Segment::Name(name.to_string()))
    }

    #[test]
    fn parses_a_wildcard_pattern() {
        let pattern = Pattern::parse("users/*").unwrap();

        assert_eq!(pattern.segs, vec![lit_name("users"), PatternSeg::Star]);
    }

    #[test]
    fn parses_nested_literals_and_indices() {
        let pattern = Pattern::parse("org/teams[0]/*").unwrap();

        assert_eq!(
            pattern.segs,
            vec![
                lit_name("org"),
                lit_name("teams"),
                PatternSeg::Lit(Segment::Index(0)),
                PatternSeg::Star,
            ]
        );
    }

    #[test]
    fn the_empty_pattern_matches_the_root() {
        let pattern = Pattern::parse("").unwrap();

        assert!(pattern.segs.is_empty());
    }

    #[test]
    fn rejects_empty_segments() {
        assert!(Pattern::parse("users//*").is_err());
        assert!(Pattern::parse("/users").is_err());
    }
}
