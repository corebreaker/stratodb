//! Opaque read and write transactions.

use crate::{
    data::{Scalar, SValue},
    engine,
    error::SdbResult,
    node::NodeKind,
    path::SPath,
    tree,
};

use redb::WriteTransaction;

/// A read-write transaction. Changes are durable only after [`WriteTxn::commit`].
pub struct WriteTxn {
    txn:   WriteTransaction,
    table: String,
}

impl WriteTxn {
    pub(crate) fn new(txn: WriteTransaction, table: String) -> Self {
        Self {
            txn,
            table,
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

    /// Commits the transaction, making its changes durable.
    pub fn commit(self) -> SdbResult<()> {
        self.txn.commit()?;
        Ok(())
    }

    /// Discards the transaction without committing. (Dropping has the same effect.)
    pub fn abort(self) {
        drop(self.txn);
    }
}
