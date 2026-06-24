//! Opaque read transaction.

use crate::{
    access::{ReadCursor, Reader},
    data::{refs::SRef, SData, SValue, Scalar},
    engine::{self, TableKey, TableValue},
    error::{SdbError, SdbResult},
    node::NodeKind,
    path::{SPath, Segment},
    tree,
    Skey,
};

use redb::{ReadOnlyTable, ReadTransaction, TableError};
use std::sync::Arc;

/// A read-only view of a table at a consistent point in time.
pub struct ReadTxn {
    txn:   ReadTransaction,
    table: String,
}

impl ReadTxn {
    pub(crate) fn new(txn: ReadTransaction, table: String) -> Self {
        Self {
            txn,
            table,
        }
    }

    /// Opens the underlying table, or `None` if it has never been written.
    fn open(&self) -> SdbResult<Option<ReadOnlyTable<TableKey, TableValue>>> {
        match self.txn.open_table(engine::data_def(&self.table)) {
            Ok(table) => Ok(Some(table)),
            Err(TableError::TableDoesNotExist(_)) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Reads the value at `path`, decoded as `V`.
    pub fn get<V: SValue>(&self, path: &str) -> SdbResult<Option<V>> {
        let path = SPath::parse(path)?;
        let Some(table) = self.open()? else {
            return Ok(None);
        };

        match tree::get_scalar(&table, &path)? {
            Some(scalar) => Ok(Some(V::from_scalar(&scalar)?)),
            None => Ok(None),
        }
    }

    /// Reads the raw scalar at `path`.
    pub fn get_scalar(&self, path: &str) -> SdbResult<Option<Scalar>> {
        let path = SPath::parse(path)?;
        let Some(table) = self.open()? else {
            return Ok(None);
        };

        tree::get_scalar(&table, &path)
    }

    /// Reports the kind of node at `path`, if any.
    pub fn kind(&self, path: &str) -> SdbResult<Option<NodeKind>> {
        let path = SPath::parse(path)?;
        let Some(table) = self.open()? else {
            return Ok(None);
        };

        tree::kind(&table, &path)
    }

    /// Returns whether a node exists at `path`.
    pub fn exists(&self, path: &str) -> SdbResult<bool> {
        Ok(self.kind(path)?.is_some())
    }

    /// Reads a typed read accessor for the value at `path`.
    pub fn fetch<'t, A: SRef<'t>>(&'t self, path: &str) -> SdbResult<A> {
        let base = SPath::parse(path)?;
        let cursor = ReadCursor::new(self);
        let key = cursor
            .resolve(&base)?
            .ok_or_else(|| SdbError::PathNotFound(base.clone()))?;

        Ok(A::open(Arc::new(cursor), base, key))
    }

    /// Recomposes a whole `T` from the subtree at `path`.
    pub fn load<T: SData>(&self, path: &str) -> SdbResult<T> {
        let base = SPath::parse(path)?;

        T::load(&ReadCursor::new(self), &base)
    }

    // -- node-level reads used by `ReadCursor` (the table is opened per call) --

    pub(crate) fn lookup_path(&self, path: &SPath) -> SdbResult<Option<Skey>> {
        let Some(table) = self.open()? else {
            return Ok(None);
        };

        tree::resolve(&table, path)
    }

    pub(crate) fn lookup_child(&self, parent: Skey, seg: &Segment) -> SdbResult<Option<Skey>> {
        let Some(table) = self.open()? else {
            return Ok(None);
        };

        tree::child_key(&table, parent, seg)
    }

    pub(crate) fn lookup_scalar(&self, key: Skey) -> SdbResult<Scalar> {
        let table = self
            .open()?
            .ok_or_else(|| SdbError::Corrupt("read on a missing table".into()))?;

        tree::scalar_at(&table, key)
    }

    pub(crate) fn lookup_scalar_at(&self, path: &SPath) -> SdbResult<Option<Scalar>> {
        let Some(table) = self.open()? else {
            return Ok(None);
        };

        tree::get_scalar(&table, path)
    }

    pub(crate) fn lookup_kind(&self, key: Skey) -> SdbResult<Option<NodeKind>> {
        let Some(table) = self.open()? else {
            return Ok(None);
        };

        tree::kind_of(&table, key)
    }

    pub(crate) fn lookup_len(&self, key: Skey) -> SdbResult<usize> {
        let table = self
            .open()?
            .ok_or_else(|| SdbError::Corrupt("read on a missing table".into()))?;

        tree::list_len(&table, key)
    }
}
