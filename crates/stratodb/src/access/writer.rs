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
}

impl Writer for WriteCursor<'_> {
    fn put_scalar(&self, path: &SPath, scalar: Scalar) -> SdbResult<()> {
        self.txn.put_scalar_path(path, scalar)
    }

    fn remove(&self, path: &SPath) -> SdbResult<bool> {
        self.txn.remove_path_at(path)
    }
}
