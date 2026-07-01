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

    /// The field names of object node `key`, in sorted order.
    fn object_keys(&self, key: Skey) -> SdbResult<Vec<String>>;
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

    fn object_keys(&self, key: Skey) -> SdbResult<Vec<String>> {
        let this = Box::as_ref(self);

        this.object_keys(key)
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

    fn object_keys(&self, key: Skey) -> SdbResult<Vec<String>> {
        let this = Arc::as_ref(self);

        this.object_keys(key)
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

    fn object_keys(&self, key: Skey) -> SdbResult<Vec<String>> {
        self.txn.lookup_object_keys(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StratoDb;

    fn p(s: &str) -> SPath {
        SPath::parse(s).unwrap()
    }

    // `R` is opaque here, so every call dispatches through `<R as Reader>`. Passing
    // an `Arc`/`Box<dyn Reader>` therefore exercises the forwarding impls — a direct
    // call on the concrete pointer would deref straight to the trait object.
    fn exercise<R: Reader>(reader: &R) {
        let a = reader.resolve(&p("a")).unwrap().unwrap();
        let x = reader.resolve(&p("a/x")).unwrap().unwrap();
        let items = reader.resolve(&p("items")).unwrap().unwrap();

        assert!(reader.child(a, &Segment::Name("x".into())).unwrap().is_some());
        assert!(
            reader
                .child_cached(a, &Segment::Name("x".into()), &p("a/x"))
                .unwrap()
                .is_some()
        );
        assert_eq!(reader.kind(a).unwrap(), Some(NodeKind::Object));
        assert_eq!(reader.kind(items).unwrap(), Some(NodeKind::List));
        assert_eq!(reader.len(items).unwrap(), 2);
        assert_eq!(reader.object_keys(a).unwrap(), vec![String::from("x")]);
        assert_eq!(reader.scalar_at(&p("a/x")).unwrap(), Some(Scalar::U32(1)));
        assert_eq!(reader.scalar(x).unwrap(), Scalar::U32(1));

        // Reading a node as the wrong kind surfaces the tree's kind guards.
        assert!(reader.scalar(a).is_err()); // object, not a leaf
        assert!(reader.len(a).is_err()); // object, not a list
        assert!(reader.object_keys(x).is_err()); // leaf, not an object

        // A cached-child lookup on a path not yet resolved takes the cache-miss
        // branch (resolve, then populate the cache).
        assert!(
            reader
                .child_cached(items, &Segment::Index(0), &p("items[0]"))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn boxed_and_arced_dyn_readers_forward_every_method() {
        let db = StratoDb::create_in_memory().unwrap();
        let table = db.open_table("t").unwrap();

        let w = table.write().unwrap();
        w.put("a/x", &1u32).unwrap();
        w.put("items[0]", &10i32).unwrap();
        w.put("items[1]", &20i32).unwrap();
        w.commit().unwrap();

        let txn = table.read().unwrap();

        let boxed: Box<dyn Reader + '_> = Box::new(ReadCursor::new(&txn));
        exercise(&boxed);

        let arced: Arc<dyn Reader + '_> = Arc::new(ReadCursor::new(&txn));
        exercise(&arced);
    }
}
