//! Opaque write transaction.

use crate::{
    access::{Reader, WriteCursor, Writer},
    data::{refs::SMut, SData, Scalar, SValue},
    db::DbInner,
    engine,
    error::{SdbError, SdbResult},
    node::NodeKind,
    path::{SPath, Segment},
    tree,
    Skey,
};

use redb::WriteTransaction;
use std::sync::{atomic::Ordering, Arc};

/// A read-write transaction. Changes are durable only after [`WriteTxn::commit`].
pub struct WriteTxn {
    txn:   WriteTransaction,
    table: String,
    inner: Arc<DbInner>,
}

impl WriteTxn {
    pub(crate) fn new(txn: WriteTransaction, table: String, inner: Arc<DbInner>) -> Self {
        Self {
            txn,
            table,
            inner,
        }
    }

    /// Stores `value` at `path`, replacing any existing subtree there.
    pub fn put<V: SValue>(&self, path: &str, value: &V) -> SdbResult<()> {
        self.put_scalar(path, value.to_scalar())
    }

    /// Stores a raw scalar at `path`, replacing any existing subtree there.
    pub fn put_scalar(&self, path: &str, scalar: Scalar) -> SdbResult<()> {
        let path = SPath::parse(path)?;
        let mut table = self.txn.open_table(engine::data_def(&self.table))?;

        tree::put_scalar(&mut table, &path, scalar)
    }

    /// Decomposes and stores a whole `value` at `path`, replacing any subtree there.
    pub fn store<T: SData>(&self, path: &str, value: &T) -> SdbResult<()> {
        let base = SPath::parse(path)?;
        let cursor = WriteCursor::new(self);
        cursor.remove(&base)?;

        value.store(&cursor, &base)
    }

    /// Removes the subtree at `path`, returning whether anything was removed.
    pub fn remove(&self, path: &str) -> SdbResult<bool> {
        let path = SPath::parse(path)?;
        let mut table = self.txn.open_table(engine::data_def(&self.table))?;

        tree::remove_path(&mut table, &path)
    }

    /// Reads the value at `path` within this transaction, decoded as `V`.
    pub fn get<V: SValue>(&self, path: &str) -> SdbResult<Option<V>> {
        let path = SPath::parse(path)?;
        let table = self.txn.open_table(engine::data_def(&self.table))?;

        match tree::get_scalar(&table, &path)? {
            Some(scalar) => Ok(Some(V::from_scalar(&scalar)?)),
            None => Ok(None),
        }
    }

    /// Reports the kind of node at `path`, if any.
    pub fn kind(&self, path: &str) -> SdbResult<Option<NodeKind>> {
        let path = SPath::parse(path)?;
        let table = self.txn.open_table(engine::data_def(&self.table))?;

        tree::kind(&table, &path)
    }

    /// Reads a typed write accessor for the value at `path`.
    pub fn fetch_mut<'t, A: SMut<'t>>(&'t self, path: &str) -> SdbResult<A> {
        let base = SPath::parse(path)?;
        let cursor = WriteCursor::new(self);
        let key = cursor
            .resolve(&base)?
            .ok_or_else(|| SdbError::PathNotFound(base.clone()))?;

        Ok(A::open(Arc::new(cursor), base, key))
    }

    /// Recomposes a whole `T` from the subtree at `path`.
    pub fn load<T: SData>(&self, path: &str) -> SdbResult<T> {
        let base = SPath::parse(path)?;

        T::load(&WriteCursor::new(self), &base)
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
            .version_lock
            .write()
            .map_err(|err| SdbError::CannotAccess(format!("version lock poisoned: {err}")))?;

        txn.commit()?;
        inner.generation.fetch_add(1, Ordering::Release);
        drop(guard);

        Ok(())
    }

    /// Discards the transaction without committing. (Dropping has the same effect.)
    pub fn abort(self) {
        drop(self.txn);
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
        let mut table = self.txn.open_table(engine::data_def(&self.table))?;

        tree::put_scalar(&mut table, path, scalar)
    }

    pub(crate) fn ensure_container_at(&self, path: &SPath, list: bool) -> SdbResult<Skey> {
        let mut table = self.txn.open_table(engine::data_def(&self.table))?;

        tree::ensure_container(&mut table, path, list)
    }

    pub(crate) fn list_move_at(&self, list_key: Skey, from: usize, to: usize) -> SdbResult<()> {
        let mut table = self.txn.open_table(engine::data_def(&self.table))?;

        tree::list_move(&mut table, list_key, from, to)
    }

    pub(crate) fn remove_path_at(&self, path: &SPath) -> SdbResult<bool> {
        let mut table = self.txn.open_table(engine::data_def(&self.table))?;

        tree::remove_path(&mut table, path)
    }
}
