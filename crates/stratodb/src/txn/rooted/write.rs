use super::super::WriteTxn;
use crate::{
    data::{refs::SMut, SData, SValue, Scalar},
    error::SdbResult,
    node::NodeKind,
    path::{IntoPath, SPath},
};

/// A [`WriteTxn`](super::WriteTxn) whose paths are relative to a fixed root.
pub struct RootedWrite<'a> {
    txn:  &'a WriteTxn,
    root: SPath,
}

impl<'a> RootedWrite<'a> {
    pub(crate) fn new(txn: &'a WriteTxn, root: SPath) -> Self {
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

    /// Stores `value` at `path` (relative to the root), replacing any subtree there.
    pub fn put<V: SValue>(&self, path: impl IntoPath, value: &V) -> SdbResult<()> {
        self.put_scalar(path, value.to_scalar())
    }

    /// Stores a raw scalar at `path` (relative to the root).
    pub fn put_scalar(&self, path: impl IntoPath, scalar: Scalar) -> SdbResult<()> {
        self.txn.put_scalar_path(&self.absolute_path(path)?, scalar)
    }

    /// Decomposes and stores a whole `value` at `path` (relative to the root).
    pub fn store<T: SData>(&self, path: impl IntoPath, value: &T) -> SdbResult<()> {
        self.txn.store_at(&self.absolute_path(path)?, value)
    }

    /// Removes the subtree at `path` (relative to the root), reporting if it existed.
    pub fn remove(&self, path: impl IntoPath) -> SdbResult<bool> {
        self.txn.remove_path_at(&self.absolute_path(path)?)
    }

    /// Reads the value at `path` (relative to the root), decoded as `V`.
    pub fn get<V: SValue>(&self, path: impl IntoPath) -> SdbResult<Option<V>> {
        self.txn.get_at(&self.absolute_path(path)?)
    }

    /// Reports the kind of node at `path` (relative to the root), if any.
    pub fn kind(&self, path: impl IntoPath) -> SdbResult<Option<NodeKind>> {
        self.txn.kind_at(&self.absolute_path(path)?)
    }

    /// Reads a typed write accessor for the value at `path` (relative to the root).
    pub fn fetch_mut<A: SMut<'a>>(&self, path: impl IntoPath) -> SdbResult<A> {
        self.txn.fetch_mut_at(&self.absolute_path(path)?)
    }

    /// Recomposes a whole `T` from the subtree at `path` (relative to the root).
    pub fn load<T: SData>(&self, path: impl IntoPath) -> SdbResult<T> {
        self.txn.load_at(&self.absolute_path(path)?)
    }

    fn absolute_path(&self, path: impl IntoPath) -> SdbResult<SPath> {
        Ok(self.root.join(&path.into_path()?))
    }
}
