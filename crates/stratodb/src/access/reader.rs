use crate::{
    data::Scalar,
    error::SdbResult,
    node::NodeKind,
    path::{SPath, Segment},
    txn::ReadTxn,
    Skey,
};

use std::sync::Arc;

/// Read access to the node tree, by primary key or by path.
pub trait Reader {
    /// The primary key a path resolves to, if any.
    fn resolve(&self, path: &SPath) -> SdbResult<Option<Skey>>;

    /// The child key under `parent` for `seg`, if present.
    fn child(&self, parent: Skey, seg: &Segment) -> SdbResult<Option<Skey>>;

    /// The scalar held by leaf node `key`.
    fn scalar(&self, key: Skey) -> SdbResult<Scalar>;

    /// The scalar stored at `path`, if it is a leaf.
    fn scalar_at(&self, path: &SPath) -> SdbResult<Option<Scalar>>;

    /// The kind of node `key`, if it exists.
    fn kind(&self, key: Skey) -> SdbResult<Option<NodeKind>>;

    /// The length of list node `key`.
    fn len(&self, key: Skey) -> SdbResult<usize>;
}

impl Reader for Box<dyn Reader + '_> {
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

impl Reader for Arc<dyn Reader + '_> {
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

/// A copyable read cursor bound to a read transaction.
#[derive(Clone, Copy)]
pub struct ReadCursor<'t> {
    txn: &'t ReadTxn,
}

impl<'t> ReadCursor<'t> {
    pub(crate) fn new(txn: &'t ReadTxn) -> Self {
        Self {
            txn,
        }
    }
}

impl Reader for ReadCursor<'_> {
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
