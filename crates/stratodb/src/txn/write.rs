//! Opaque write transaction.

use super::rooted::RootedWrite;
use crate::{
    access::{BoundCursor, Reader, WriteCursor, Writer},
    data::{refs::SMut, SData, Scalar, SValue},
    db::DbInner,
    engine::{self, TableKey, TableValue},
    error::{SdbError, SdbResult},
    index::{
        maintenance,
        registry::{self, IndexEntry},
    },
    node::NodeKind,
    path::{IntoPath, SPath, Segment},
    tree,
    Skey,
};

use redb::{Table, WriteTransaction};
use std::{
    cell::RefCell,
    sync::{atomic::Ordering, Arc, OnceLock},
};

/// The writable engine table holding this table's nodes and index entries.
type DataTable<'txn> = Table<'txn, TableKey, TableValue>;

/// A read-write transaction. Changes are durable only after [`WriteTxn::commit`].
pub struct WriteTxn {
    txn:     WriteTransaction,
    table:   String,
    inner:   Arc<DbInner>,
    /// This table's indexes, loaded from `$metadata` on first mutation and reused
    /// for the rest of the transaction (the set cannot change mid-transaction).
    /// `OnceLock` rather than `OnceCell` so the transaction stays `Sync`.
    indexes: OnceLock<Vec<IndexEntry>>,
}

impl WriteTxn {
    pub(crate) fn new(txn: WriteTransaction, table: String, inner: Arc<DbInner>) -> Self {
        Self {
            txn,
            table,
            inner,
            indexes: OnceLock::new(),
        }
    }

    /// Stores `value` at `path`, replacing any existing subtree there.
    pub fn put<V: SValue>(&self, path: impl IntoPath, value: &V) -> SdbResult<()> {
        self.put_scalar(path, value.to_scalar())
    }

    /// Stores a raw scalar at `path`, replacing any existing subtree there.
    pub fn put_scalar(&self, path: impl IntoPath, scalar: Scalar) -> SdbResult<()> {
        self.put_scalar_path(&path.into_path()?, scalar)
    }

    /// Decomposes and stores a whole `value` at `path`, replacing any subtree there.
    pub fn store<T: SData>(&self, path: impl IntoPath, value: &T) -> SdbResult<()> {
        self.store_at(&path.into_path()?, value)
    }

    /// Removes the subtree at `path`, returning whether anything was removed.
    pub fn remove(&self, path: impl IntoPath) -> SdbResult<bool> {
        self.remove_path_at(&path.into_path()?)
    }

    /// Reads the value at `path` within this transaction, decoded as `V`.
    pub fn get<V: SValue>(&self, path: impl IntoPath) -> SdbResult<Option<V>> {
        self.get_at(&path.into_path()?)
    }

    /// Reports the kind of node at `path`, if any.
    pub fn kind(&self, path: impl IntoPath) -> SdbResult<Option<NodeKind>> {
        self.kind_at(&path.into_path()?)
    }

    /// Reads a typed write accessor for the value at `path`.
    pub fn fetch_mut<'t, A: SMut<'t>>(&'t self, path: impl IntoPath) -> SdbResult<A> {
        self.fetch_mut_at(&path.into_path()?)
    }

    /// Recomposes a whole `T` from the subtree at `path`.
    pub fn load<T: SData>(&self, path: impl IntoPath) -> SdbResult<T> {
        self.load_at(&path.into_path()?)
    }

    /// Returns a view of this transaction whose paths are relative to `root`.
    ///
    /// Every path passed to the returned [`RootedWrite`] resolves as `root` then
    /// the path. The view borrows the transaction, so it must be dropped before
    /// [`commit`](WriteTxn::commit) (like a write accessor).
    pub fn rooted(&self, root: impl IntoPath) -> SdbResult<RootedWrite<'_>> {
        Ok(RootedWrite::new(self, root.into_path()?))
    }

    // -- path-addressed cores (shared by the `&str` API and rooted views) --

    pub(crate) fn store_at<T: SData>(&self, base: &SPath, value: &T) -> SdbResult<()> {
        let Some((parent_path, last)) = base.split_last() else {
            // Storing at the table root: nothing to anchor to, so take the plain path.
            let cursor = WriteCursor::new(self);
            cursor.remove(base)?;

            return value.store(&cursor, base);
        };

        // One index-maintenance bracket and one open table handle for the whole
        // entity: clear the old subtree, resolve the entity's parent once, then let
        // every field write resolve relative to that anchor instead of the root.
        let rel = segment_path(last);
        self.reindex_around(base, |table| {
            tree::remove_path(table, base)?;
            let anchor = tree::ensure_container(table, &parent_path, matches!(last, Segment::Index(_)))?;

            let cell = RefCell::new(table);
            value.store(&BoundCursor::new(&cell, anchor), &rel)
        })
    }

    pub(crate) fn get_at<V: SValue>(&self, path: &SPath) -> SdbResult<Option<V>> {
        let table = self.txn.open_table(engine::data_def(&self.table))?;

        match tree::get_scalar(&table, path)? {
            Some(scalar) => Ok(Some(V::from_scalar(&scalar)?)),
            None => Ok(None),
        }
    }

    pub(crate) fn kind_at(&self, path: &SPath) -> SdbResult<Option<NodeKind>> {
        let table = self.txn.open_table(engine::data_def(&self.table))?;

        tree::kind(&table, path)
    }

    pub(crate) fn fetch_mut_at<'t, A: SMut<'t>>(&'t self, base: &SPath) -> SdbResult<A> {
        let cursor = WriteCursor::new(self);
        let key = cursor
            .resolve(base)?
            .ok_or_else(|| SdbError::PathNotFound(base.clone()))?;

        Ok(A::open(Arc::new(cursor), base.clone(), key))
    }

    pub(crate) fn load_at<T: SData>(&self, base: &SPath) -> SdbResult<T> {
        T::load(&WriteCursor::new(self), base)
    }

    /// Commits the transaction, making its changes durable and bumping the
    /// database generation so cached path resolutions from older snapshots are no
    /// longer served.
    pub fn commit(self) -> SdbResult<()> {
        let WriteTxn {
            txn,
            inner,
            ..
        } = self;

        let guard = inner
            .version_lock()
            .write()
            .map_err(|err| SdbError::CannotAccess(format!("version lock poisoned: {err}")))?;

        txn.commit()?;
        inner.generation().fetch_add(1, Ordering::Release);
        drop(guard);

        Ok(())
    }

    /// Discards the transaction without committing. (Dropping has the same effect.)
    pub fn abort(self) {
        drop(self.txn);
    }

    // -- index maintenance --

    /// This table's indexes, loaded once and cached for the transaction.
    fn indexes(&self) -> SdbResult<&[IndexEntry]> {
        if let Some(indexes) = self.indexes.get() {
            return Ok(indexes);
        }

        let loaded = {
            let meta = self.txn.open_table(engine::META_TABLE)?;

            registry::for_table(&meta, &self.table)?
        };

        Ok(self.indexes.get_or_init(|| loaded))
    }

    /// Runs a mutation at `scope`, keeping the table's indexes consistent: the
    /// entities `scope` could affect are de-indexed before the change and
    /// re-indexed after it. Tables with no indexes take a direct, zero-overhead
    /// path.
    pub(crate) fn reindex_around<R>(
        &self,
        scope: &SPath,
        apply: impl FnOnce(&mut DataTable<'_>) -> SdbResult<R>,
    ) -> SdbResult<R> {
        let indexes = self.indexes()?;
        let mut table = self.txn.open_table(engine::data_def(&self.table))?;

        if indexes.is_empty() {
            return apply(&mut table);
        }

        maintenance::delete(&mut table, indexes, scope)?;
        let result = apply(&mut table)?;
        maintenance::insert(&mut table, indexes, scope)?;

        Ok(result)
    }

    // -- node-level access used by `WriteCursor` (the write table is opened per call) --

    pub(crate) fn lookup_path(&self, path: &SPath) -> SdbResult<Option<Skey>> {
        let table = self.txn.open_table(engine::data_def(&self.table))?;

        tree::resolve(&table, path)
    }

    pub(crate) fn lookup_child(&self, parent: Skey, seg: &Segment) -> SdbResult<Option<Skey>> {
        let table = self.txn.open_table(engine::data_def(&self.table))?;

        tree::child_key(&table, parent, seg)
    }

    pub(crate) fn lookup_scalar(&self, key: Skey) -> SdbResult<Scalar> {
        let table = self.txn.open_table(engine::data_def(&self.table))?;

        tree::scalar_at(&table, key)
    }

    pub(crate) fn lookup_scalar_at(&self, path: &SPath) -> SdbResult<Option<Scalar>> {
        let table = self.txn.open_table(engine::data_def(&self.table))?;

        tree::get_scalar(&table, path)
    }

    pub(crate) fn lookup_kind(&self, key: Skey) -> SdbResult<Option<NodeKind>> {
        let table = self.txn.open_table(engine::data_def(&self.table))?;

        tree::kind_of(&table, key)
    }

    pub(crate) fn lookup_len(&self, key: Skey) -> SdbResult<usize> {
        let table = self.txn.open_table(engine::data_def(&self.table))?;

        tree::list_len(&table, key)
    }

    pub(crate) fn lookup_object_keys(&self, key: Skey) -> SdbResult<Vec<String>> {
        let table = self.txn.open_table(engine::data_def(&self.table))?;

        tree::object_keys(&table, key)
    }

    pub(crate) fn put_scalar_path(&self, path: &SPath, scalar: Scalar) -> SdbResult<()> {
        self.reindex_around(path, |table| tree::put_scalar(table, path, scalar))
    }

    pub(crate) fn ensure_container_at(&self, path: &SPath, list: bool) -> SdbResult<Skey> {
        self.reindex_around(path, |table| tree::ensure_container(table, path, list))
    }

    // List reordering preserves every element's key and column values, so no index
    // entry can change; these skip index maintenance.
    pub(crate) fn list_move_at(&self, list_key: Skey, from: usize, to: usize) -> SdbResult<()> {
        let mut table = self.txn.open_table(engine::data_def(&self.table))?;

        tree::list_move(&mut table, list_key, from, to)
    }

    pub(crate) fn list_swap_at(&self, list_key: Skey, i: usize, j: usize) -> SdbResult<()> {
        let mut table = self.txn.open_table(engine::data_def(&self.table))?;

        tree::list_swap(&mut table, list_key, i, j)
    }

    pub(crate) fn clear_children_at(&self, path: &SPath, key: Skey) -> SdbResult<()> {
        self.reindex_around(path, |table| tree::clear_children(table, key))
    }

    pub(crate) fn remove_path_at(&self, path: &SPath) -> SdbResult<bool> {
        self.reindex_around(path, |table| tree::remove_path(table, path))
    }
}

/// The single-segment path holding just `seg`, used to drive a bound store one
/// segment below its anchor.
pub(crate) fn segment_path(seg: &Segment) -> SPath {
    match seg {
        Segment::Name(name) => SPath::root().child_name(name),
        Segment::Index(index) => SPath::root().child_index(*index),
    }
}
