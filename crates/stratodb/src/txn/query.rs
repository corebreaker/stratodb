//! The secondary-index query builder.
//!
//! [`ReadTxn::query`](super::ReadTxn::query) returns an [`IndexQuery`] that runs
//! an exact or **prefix** match against an index and recomposes each hit as a
//! `T: SData`. A query carries:
//!
//! - a **prefix** of column values (in the index's column order). Fewer values than the index has columns is a prefix
//!   match — every entity whose leading columns equal the prefix; an empty prefix matches every indexed entity. More
//!   values than columns is an [`IndexArity`](crate::SdbError::IndexArity) error.
//! - a **direction**: results come back in index order by default (ascending by the encoded key, honoring each column's
//!   ASC/DESC), or reversed.
//! - an optional **root**: when set (by [`RootedRead`](super::RootedRead)), only entities at or under that path are
//!   kept.

use super::ReadTxn;
use crate::{
    data::{SData, Scalar},
    error::SdbResult,
    path::SPath,
};

/// A pending index query (see the [module docs](self)). Build it up, then
/// [`run`](IndexQuery::run).
pub struct IndexQuery<'t> {
    txn:     &'t ReadTxn,
    index:   String,
    prefix:  Vec<Scalar>,
    reverse: bool,
    root:    SPath,
}

impl<'t> IndexQuery<'t> {
    pub(crate) fn new(txn: &'t ReadTxn, index: &str) -> Self {
        Self {
            txn,
            index: index.to_string(),
            prefix: Vec::new(),
            reverse: false,
            root: SPath::root(),
        }
    }

    /// Matches entities whose leading columns equal `values` (in column order).
    /// Fewer values than the index has columns is a prefix match; the full set is
    /// an exact match; an empty prefix (the default) matches every indexed entity.
    pub fn prefixed(self, values: &[Scalar]) -> Self {
        Self {
            prefix: values.to_vec(),
            ..self
        }
    }

    /// Returns results in reverse index order instead of forward.
    pub fn reversed(self) -> Self {
        Self {
            reverse: true,
            ..self
        }
    }

    /// Keeps only entities at or under `root`. (Set automatically by a rooted
    /// view; see [`RootedRead::query`](super::RootedRead::query).)
    pub fn under(self, root: SPath) -> Self {
        Self {
            root,
            ..self
        }
    }

    /// Runs the query, recomposing each matched entity as a `T`.
    pub fn run<T: SData>(self) -> SdbResult<Vec<T>> {
        self.txn
            .execute_query(&self.index, &self.prefix, self.reverse, &self.root)
    }
}
