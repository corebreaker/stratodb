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

    /// Like [`child`](Reader::child), but may serve the answer from a shared
    /// path-resolution cache.
    ///
    /// `child_path` is the child node's full path (`parent`'s path followed by
    /// `seg`); a cache-backed reader uses it as the cache key, so navigating to
    /// the same node again — from any accessor or any transaction of the same
    /// database generation — costs no I/O. The default implementation ignores
    /// `child_path` and simply performs the lookup, which is what write cursors
    /// rely on (a writer sees its own uncommitted changes, so its resolutions
    /// must never be cached).
    fn child_cached(&self, parent: Skey, seg: &Segment, child_path: &SPath) -> SdbResult<Option<Skey>> {
        let _ = child_path;

        self.child(parent, seg)
    }

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

    fn child_cached(&self, parent: Skey, seg: &Segment, child_path: &SPath) -> SdbResult<Option<Skey>> {
        self.txn.lookup_child_cached(parent, seg, child_path)
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
