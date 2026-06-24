use super::super::ReadTxn;
use crate::{
    data::{refs::SRef, SData, SValue, Scalar},
    error::SdbResult,
    node::NodeKind,
    path::SPath,
};

/// A [`ReadTxn`](super::ReadTxn) whose paths are relative to a fixed root.
pub struct RootedRead<'a> {
    txn:  &'a ReadTxn,
    root: SPath,
}

impl<'a> RootedRead<'a> {
    pub(crate) fn new(txn: &'a ReadTxn, root: SPath) -> Self {
        Self {
            txn,
            root,
        }
    }

    /// The root this view is anchored at.
    pub fn root(&self) -> &SPath {
        &self.root
    }

    /// A further view, rooted at `sub` relative to this view's root.
    pub fn rooted(&self, sub: SPath) -> Self {
        Self {
            txn:  self.txn,
            root: self.root.join(&sub),
        }
    }

    /// Reads the value at `path` (relative to the root), decoded as `V`.
    pub fn get<V: SValue>(&self, path: &str) -> SdbResult<Option<V>> {
        self.txn.get_at(&self.abs(path)?)
    }

    /// Reads the raw scalar at `path` (relative to the root).
    pub fn get_scalar(&self, path: &str) -> SdbResult<Option<Scalar>> {
        self.txn.lookup_scalar_at(&self.abs(path)?)
    }

    /// Reports the kind of node at `path` (relative to the root), if any.
    pub fn kind(&self, path: &str) -> SdbResult<Option<NodeKind>> {
        self.txn.kind_at(&self.abs(path)?)
    }

    /// Returns whether a node exists at `path` (relative to the root).
    pub fn exists(&self, path: &str) -> SdbResult<bool> {
        Ok(self.kind(path)?.is_some())
    }

    /// Reads a typed read accessor for the value at `path` (relative to the root).
    pub fn fetch<A: SRef<'a>>(&self, path: &str) -> SdbResult<A> {
        self.txn.fetch_at(&self.abs(path)?)
    }

    /// Recomposes a whole `T` from the subtree at `path` (relative to the root).
    pub fn load<T: SData>(&self, path: &str) -> SdbResult<T> {
        self.txn.load_at(&self.abs(path)?)
    }

    fn abs(&self, path: &str) -> SdbResult<SPath> {
        Ok(self.root.join(&SPath::parse(path)?))
    }
}
