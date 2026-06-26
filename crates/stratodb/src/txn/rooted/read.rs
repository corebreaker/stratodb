use super::super::{IndexQuery, ReadTxn};
use crate::{
    data::{refs::SRef, SData, SValue, Scalar},
    error::SdbResult,
    node::NodeKind,
    path::{IntoPath, SPath},
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
    pub fn rooted(&self, sub: impl IntoPath) -> SdbResult<Self> {
        Ok(Self {
            txn:  self.txn,
            root: self.root.join(&sub.into_path()?),
        })
    }

    /// Reads the value at `path` (relative to the root), decoded as `V`.
    pub fn get<V: SValue>(&self, path: impl IntoPath) -> SdbResult<Option<V>> {
        self.txn.get_at(&self.absolute_path(path)?)
    }

    /// Reads the raw scalar at `path` (relative to the root).
    pub fn get_scalar(&self, path: impl IntoPath) -> SdbResult<Option<Scalar>> {
        self.txn.lookup_scalar_at(&self.absolute_path(path)?)
    }

    /// Reports the kind of node at `path` (relative to the root), if any.
    pub fn kind(&self, path: impl IntoPath) -> SdbResult<Option<NodeKind>> {
        self.txn.kind_at(&self.absolute_path(path)?)
    }

    /// Returns whether a node exists at `path` (relative to the root).
    pub fn exists(&self, path: impl IntoPath) -> SdbResult<bool> {
        Ok(self.kind(path)?.is_some())
    }

    /// Reads a typed read accessor for the value at `path` (relative to the root).
    pub fn fetch<A: SRef<'a>>(&self, path: impl IntoPath) -> SdbResult<A> {
        self.txn.fetch_at(&self.absolute_path(path)?)
    }

    /// Recomposes a whole `T` from the subtree at `path` (relative to the root).
    pub fn load<T: SData>(&self, path: impl IntoPath) -> SdbResult<T> {
        self.txn.load_at(&self.absolute_path(path)?)
    }

    /// Starts an [`IndexQuery`] scoped to this view's root: only entities at or
    /// under the root are kept. See [`ReadTxn::query`](super::ReadTxn::query).
    pub fn query(&self, index: &str) -> IndexQuery<'a> {
        self.txn.query(index).under(self.root.clone())
    }

    /// Finds the entities an index points at, keeping only those at or under this
    /// view's root, each recomposed as a `T`.
    ///
    /// Like [`ReadTxn::find`](super::ReadTxn::find) but scoped to the view (the
    /// root itself counts); the index is table-global, this filters its matches.
    pub fn find<T: SData>(&self, index: &str, values: &[Scalar]) -> SdbResult<Vec<T>> {
        self.query(index).prefixed(values).run()
    }

    fn absolute_path(&self, path: impl IntoPath) -> SdbResult<SPath> {
        Ok(self.root.join(&path.into_path()?))
    }
}
