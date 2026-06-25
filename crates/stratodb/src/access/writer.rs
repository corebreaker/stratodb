use super::Reader;
use crate::{
    data::Scalar,
    error::SdbResult,
    node::NodeKind,
    path::{SPath, Segment},
    txn::WriteTxn,
    Skey,
};

use std::sync::Arc;

/// Read/write access to the node tree.
pub trait Writer: Reader {
    /// Stores `scalar` at `path`, replacing any existing subtree there.
    fn put_scalar(&self, path: &SPath, scalar: Scalar) -> SdbResult<()>;

    /// Removes the subtree at `path`, returning whether anything was removed.
    fn remove(&self, path: &SPath) -> SdbResult<bool>;

    /// Ensures a container node exists at `path` (a list when `list`, otherwise an
    /// object), creating ancestors as needed, and returns its key. Used so that
    /// empty containers still have an addressable node.
    fn ensure_container(&self, path: &SPath, list: bool) -> SdbResult<Skey>;

    /// Moves a list element from `from` to `to` within list node `list_key`,
    /// reordering positions without touching the moved subtree (keys are stable).
    fn list_move(&self, list_key: Skey, from: usize, to: usize) -> SdbResult<()>;

    /// Swaps the elements at `i` and `j` within list node `list_key` (keys stable).
    fn list_swap(&self, list_key: Skey, i: usize, j: usize) -> SdbResult<()>;

    /// Removes every child of the container at `path` (node `key`), leaving it
    /// empty. `path` lets index maintenance de-index the cleared entities.
    fn clear_children(&self, path: &SPath, key: Skey) -> SdbResult<()>;
}

impl Reader for Box<dyn Writer + '_> {
    fn resolve(&self, path: &SPath) -> SdbResult<Option<Skey>> {
        let this = Box::as_ref(self);

        this.resolve(path)
    }

    fn child(&self, parent: Skey, seg: &Segment) -> SdbResult<Option<Skey>> {
        let this = Box::as_ref(self);

        this.child(parent, seg)
    }

    fn child_cached(&self, parent: Skey, seg: &Segment, child_path: &SPath) -> SdbResult<Option<Skey>> {
        let this = Box::as_ref(self);

        this.child_cached(parent, seg, child_path)
    }

    fn scalar(&self, key: Skey) -> SdbResult<Scalar> {
        let this = Box::as_ref(self);

        this.scalar(key)
    }

    fn scalar_at(&self, path: &SPath) -> SdbResult<Option<Scalar>> {
        let this = Box::as_ref(self);

        this.scalar_at(path)
    }

    fn kind(&self, key: Skey) -> SdbResult<Option<NodeKind>> {
        let this = Box::as_ref(self);

        this.kind(key)
    }

    fn len(&self, key: Skey) -> SdbResult<usize> {
        let this = Box::as_ref(self);

        this.len(key)
    }

    fn object_keys(&self, key: Skey) -> SdbResult<Vec<String>> {
        let this = Box::as_ref(self);

        this.object_keys(key)
    }
}

impl Writer for Box<dyn Writer + '_> {
    fn put_scalar(&self, path: &SPath, scalar: Scalar) -> SdbResult<()> {
        let this = Box::as_ref(self);

        this.put_scalar(path, scalar)
    }

    fn remove(&self, path: &SPath) -> SdbResult<bool> {
        let this = Box::as_ref(self);

        this.remove(path)
    }

    fn ensure_container(&self, path: &SPath, list: bool) -> SdbResult<Skey> {
        let this = Box::as_ref(self);

        this.ensure_container(path, list)
    }

    fn list_move(&self, list_key: Skey, from: usize, to: usize) -> SdbResult<()> {
        let this = Box::as_ref(self);

        this.list_move(list_key, from, to)
    }

    fn list_swap(&self, list_key: Skey, i: usize, j: usize) -> SdbResult<()> {
        let this = Box::as_ref(self);

        this.list_swap(list_key, i, j)
    }

    fn clear_children(&self, path: &SPath, key: Skey) -> SdbResult<()> {
        let this = Box::as_ref(self);

        this.clear_children(path, key)
    }
}

impl Reader for Arc<dyn Writer + '_> {
    fn resolve(&self, path: &SPath) -> SdbResult<Option<Skey>> {
        let this = Arc::as_ref(self);

        this.resolve(path)
    }

    fn child(&self, parent: Skey, seg: &Segment) -> SdbResult<Option<Skey>> {
        let this = Arc::as_ref(self);

        this.child(parent, seg)
    }

    fn child_cached(&self, parent: Skey, seg: &Segment, child_path: &SPath) -> SdbResult<Option<Skey>> {
        let this = Arc::as_ref(self);

        this.child_cached(parent, seg, child_path)
    }

    fn scalar(&self, key: Skey) -> SdbResult<Scalar> {
        let this = Arc::as_ref(self);

        this.scalar(key)
    }

    fn scalar_at(&self, path: &SPath) -> SdbResult<Option<Scalar>> {
        let this = Arc::as_ref(self);

        this.scalar_at(path)
    }

    fn kind(&self, key: Skey) -> SdbResult<Option<NodeKind>> {
        let this = Arc::as_ref(self);

        this.kind(key)
    }

    fn len(&self, key: Skey) -> SdbResult<usize> {
        let this = Arc::as_ref(self);

        this.len(key)
    }

    fn object_keys(&self, key: Skey) -> SdbResult<Vec<String>> {
        let this = Arc::as_ref(self);

        this.object_keys(key)
    }
}

impl Writer for Arc<dyn Writer + '_> {
    fn put_scalar(&self, path: &SPath, scalar: Scalar) -> SdbResult<()> {
        let this = Arc::as_ref(self);

        this.put_scalar(path, scalar)
    }

    fn remove(&self, path: &SPath) -> SdbResult<bool> {
        let this = Arc::as_ref(self);

        this.remove(path)
    }

    fn ensure_container(&self, path: &SPath, list: bool) -> SdbResult<Skey> {
        let this = Arc::as_ref(self);

        this.ensure_container(path, list)
    }

    fn list_move(&self, list_key: Skey, from: usize, to: usize) -> SdbResult<()> {
        let this = Arc::as_ref(self);

        this.list_move(list_key, from, to)
    }

    fn list_swap(&self, list_key: Skey, i: usize, j: usize) -> SdbResult<()> {
        let this = Arc::as_ref(self);

        this.list_swap(list_key, i, j)
    }

    fn clear_children(&self, path: &SPath, key: Skey) -> SdbResult<()> {
        let this = Arc::as_ref(self);

        this.clear_children(path, key)
    }
}

/// A copyable read/write cursor bound to a write transaction.
#[derive(Clone, Copy)]
pub(crate) struct WriteCursor<'t> {
    txn: &'t WriteTxn,
}

impl<'t> WriteCursor<'t> {
    pub(crate) fn new(txn: &'t WriteTxn) -> Self {
        Self {
            txn,
        }
    }
}

impl Reader for WriteCursor<'_> {
    fn resolve(&self, path: &SPath) -> SdbResult<Option<Skey>> {
        self.txn.lookup_path(path)
    }

    fn child(&self, parent: Skey, seg: &Segment) -> SdbResult<Option<Skey>> {
        self.txn.lookup_child(parent, seg)
    }

    fn scalar(&self, key: Skey) -> SdbResult<Scalar> {
        self.txn.lookup_scalar(key)
    }

    fn scalar_at(&self, path: &SPath) -> SdbResult<Option<Scalar>> {
        self.txn.lookup_scalar_at(path)
    }

    fn kind(&self, key: Skey) -> SdbResult<Option<NodeKind>> {
        self.txn.lookup_kind(key)
    }

    fn len(&self, key: Skey) -> SdbResult<usize> {
        self.txn.lookup_len(key)
    }

    fn object_keys(&self, key: Skey) -> SdbResult<Vec<String>> {
        self.txn.lookup_object_keys(key)
    }
}

impl Writer for WriteCursor<'_> {
    fn put_scalar(&self, path: &SPath, scalar: Scalar) -> SdbResult<()> {
        self.txn.put_scalar_path(path, scalar)
    }

    fn remove(&self, path: &SPath) -> SdbResult<bool> {
        self.txn.remove_path_at(path)
    }

    fn ensure_container(&self, path: &SPath, list: bool) -> SdbResult<Skey> {
        self.txn.ensure_container_at(path, list)
    }

    fn list_move(&self, list_key: Skey, from: usize, to: usize) -> SdbResult<()> {
        self.txn.list_move_at(list_key, from, to)
    }

    fn list_swap(&self, list_key: Skey, i: usize, j: usize) -> SdbResult<()> {
        self.txn.list_swap_at(list_key, i, j)
    }

    fn clear_children(&self, path: &SPath, key: Skey) -> SdbResult<()> {
        self.txn.clear_children_at(path, key)
    }
}
