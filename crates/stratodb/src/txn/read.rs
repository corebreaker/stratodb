//! Opaque read transaction.

use crate::{
    access::{ReadCursor, Reader},
    cache::PathCache,
    data::{refs::SRef, SData, SValue, Scalar},
    engine::{self, TableKey, TableValue},
    error::{SdbError, SdbResult},
    node::{Node, NodeKind},
    path::{SPath, Segment},
    tree,
    Skey,
};

use redb::{ReadOnlyTable, ReadTransaction, TableError};
use std::sync::Arc;

/// A read-only view of a table at a consistent point in time.
pub struct ReadTxn {
    txn:        ReadTransaction,
    table:      String,
    generation: u64,
    cache:      Arc<PathCache>,
}

impl ReadTxn {
    pub(crate) fn new(txn: ReadTransaction, table: String, generation: u64, cache: Arc<PathCache>) -> Self {
        Self {
            txn,
            table,
            generation,
            cache,
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
        match self.scalar_at_path(&path)? {
            Some(scalar) => Ok(Some(V::from_scalar(&scalar)?)),
            None => Ok(None),
        }
    }

    /// Reads the raw scalar at `path`.
    pub fn get_scalar(&self, path: &str) -> SdbResult<Option<Scalar>> {
        let path = SPath::parse(path)?;
        self.scalar_at_path(&path)
    }

    /// Reports the kind of node at `path`, if any.
    pub fn kind(&self, path: &str) -> SdbResult<Option<NodeKind>> {
        let path = SPath::parse(path)?;
        let Some(key) = self.lookup_path(&path)? else {
            return Ok(None);
        };

        self.lookup_kind(key)
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

    // -- resolution (cached) and node-level reads used by `ReadCursor` --

    /// Resolves `path` to a key, consulting (and populating) the shared cache.
    pub(crate) fn lookup_path(&self, path: &SPath) -> SdbResult<Option<Skey>> {
        if let Some(key) = self.cache.get(self.generation, path)? {
            return Ok(Some(key));
        }

        let Some(table) = self.open()? else {
            return Ok(None);
        };

        let resolved = tree::resolve(&table, path)?;
        if let Some(key) = resolved {
            self.cache.put(self.generation, path, key)?;
        }

        Ok(resolved)
    }

    /// Reads the scalar at `path` (resolving through the cache); errors if the node
    /// there is not a leaf.
    fn scalar_at_path(&self, path: &SPath) -> SdbResult<Option<Scalar>> {
        let Some(key) = self.lookup_path(path)? else {
            return Ok(None);
        };
        let Some(table) = self.open()? else {
            return Ok(None);
        };

        match tree::read_node(&table, key)? {
            Some(Node::Leaf(scalar)) => Ok(Some(scalar)),
            Some(other) => Err(SdbError::UnexpectedNode {
                path:     path.clone(),
                expected: "leaf",
                found:    other.kind().as_str(),
            }),
            None => Err(SdbError::Corrupt("path resolves to a missing node".into())),
        }
    }

    pub(crate) fn lookup_child(&self, parent: Skey, seg: &Segment) -> SdbResult<Option<Skey>> {
        let Some(table) = self.open()? else {
            return Ok(None);
        };

        tree::child_key(&table, parent, seg)
    }

    /// Resolves `parent`'s `seg` child, consulting and populating the shared cache
    /// under the child's full path. `child_path` must equal `parent`'s path
    /// followed by `seg`; since `parent` was itself resolved at this snapshot's
    /// generation, the one-hop result is the correct resolution of `child_path`.
    pub(crate) fn lookup_child_cached(
        &self,
        parent: Skey,
        seg: &Segment,
        child_path: &SPath,
    ) -> SdbResult<Option<Skey>> {
        if let Some(key) = self.cache.get(self.generation, child_path)? {
            return Ok(Some(key));
        }

        let Some(table) = self.open()? else {
            return Ok(None);
        };

        let resolved = tree::child_key(&table, parent, seg)?;
        if let Some(key) = resolved {
            self.cache.put(self.generation, child_path, key)?;
        }

        Ok(resolved)
    }

    pub(crate) fn lookup_scalar(&self, key: Skey) -> SdbResult<Scalar> {
        let table = self
            .open()?
            .ok_or_else(|| SdbError::Corrupt("read on a missing table".into()))?;

        tree::scalar_at(&table, key)
    }

    pub(crate) fn lookup_scalar_at(&self, path: &SPath) -> SdbResult<Option<Scalar>> {
        self.scalar_at_path(path)
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

    pub(crate) fn lookup_object_keys(&self, key: Skey) -> SdbResult<Vec<String>> {
        let table = self
            .open()?
            .ok_or_else(|| SdbError::Corrupt("read on a missing table".into()))?;

        tree::object_keys(&table, key)
    }
}
