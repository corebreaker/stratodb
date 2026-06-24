//! Opaque read transaction.

use super::rooted::RootedRead;
use crate::{
    access::{ReadCursor, Reader, Rooted},
    cache::PathCache,
    data::{refs::SRef, SData, SValue, Scalar},
    engine::{self, TableKey, TableValue},
    error::{SdbError, SdbResult},
    index::{registry, IndexId},
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
        self.get_at(&SPath::parse(path)?)
    }

    /// Reads the raw scalar at `path`.
    pub fn get_scalar(&self, path: &str) -> SdbResult<Option<Scalar>> {
        self.scalar_at_path(&SPath::parse(path)?)
    }

    /// Reports the kind of node at `path`, if any.
    pub fn kind(&self, path: &str) -> SdbResult<Option<NodeKind>> {
        self.kind_at(&SPath::parse(path)?)
    }

    /// Returns whether a node exists at `path`.
    pub fn exists(&self, path: &str) -> SdbResult<bool> {
        Ok(self.kind(path)?.is_some())
    }

    /// Reads a typed read accessor for the value at `path`.
    pub fn fetch<'t, A: SRef<'t>>(&'t self, path: &str) -> SdbResult<A> {
        self.fetch_at(&SPath::parse(path)?)
    }

    /// Recomposes a whole `T` from the subtree at `path`.
    pub fn load<T: SData>(&self, path: &str) -> SdbResult<T> {
        self.load_at(&SPath::parse(path)?)
    }

    /// Returns a view of this transaction whose paths are relative to `root`.
    ///
    /// Every path passed to the returned [`RootedRead`] resolves as `root` then
    /// the path, so `txn.rooted(SPath::parse("users/alice")?).get("age")` reads
    /// `users/alice/age`. The view borrows the transaction. Index queries
    /// (`find`) are not re-rooted — indexes are defined over the whole table.
    pub fn rooted(&self, root: SPath) -> RootedRead<'_> {
        RootedRead::new(self, root)
    }

    // -- path-addressed cores (shared by the `&str` API and rooted views) --

    pub(crate) fn get_at<V: SValue>(&self, path: &SPath) -> SdbResult<Option<V>> {
        match self.scalar_at_path(path)? {
            Some(scalar) => Ok(Some(V::from_scalar(&scalar)?)),
            None => Ok(None),
        }
    }

    pub(crate) fn kind_at(&self, path: &SPath) -> SdbResult<Option<NodeKind>> {
        let Some(key) = self.lookup_path(path)? else {
            return Ok(None);
        };

        self.lookup_kind(key)
    }

    pub(crate) fn fetch_at<'t, A: SRef<'t>>(&'t self, base: &SPath) -> SdbResult<A> {
        let cursor = ReadCursor::new(self);
        let key = cursor
            .resolve(base)?
            .ok_or_else(|| SdbError::PathNotFound(base.clone()))?;

        Ok(A::open(Arc::new(cursor), base.clone(), key))
    }

    pub(crate) fn load_at<T: SData>(&self, base: &SPath) -> SdbResult<T> {
        T::load(&ReadCursor::new(self), base)
    }

    /// Finds the entities an index points at for an exact match on `values`,
    /// recomposing each as a `T`.
    ///
    /// `values` gives one scalar per index column, in the index's column order —
    /// the full key, so this is an exact (equality) lookup on every column. The
    /// results are returned in index order (ascending by the encoded key, honoring
    /// each column's direction). Errors with [`SdbError::IndexNotFound`] for an
    /// unknown index and [`SdbError::IndexArity`] for the wrong number of values.
    pub fn find<T: SData>(&self, index: &str, values: &[Scalar]) -> SdbResult<Vec<T>> {
        let entry = {
            let meta = self.txn.open_table(engine::META_TABLE)?;

            registry::lookup(&meta, &self.table, index)?
        }
        .ok_or_else(|| SdbError::IndexNotFound {
            index: index.to_string(),
        })?;

        let def = entry.def();
        if values.len() != def.columns().len() {
            return Err(SdbError::IndexArity {
                index:    index.to_string(),
                expected: def.columns().len(),
                got:      values.len(),
            });
        }

        let id = entry.id();
        let cols = def.encode_columns(values);

        let Some(table) = self.open()? else {
            return Ok(Vec::new());
        };

        let entities = if def.unique() {
            unique_match(&table, id, cols)?
        } else {
            duplicate_matches(&table, id, cols)?
        };

        // Each match is addressed by its (stable) key; re-root a reader there so
        // the path-based loader recomposes the entity from its own subtree.
        let cursor = ReadCursor::new(self);
        entities
            .into_iter()
            .map(|entity| T::load(&Rooted::new(&cursor, entity), &SPath::root()))
            .collect()
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

/// The single entity a unique index stores for `cols` (in the entry's value), or
/// none if there is no such entry.
fn unique_match(table: &ReadOnlyTable<TableKey, TableValue>, id: IndexId, cols: Vec<u8>) -> SdbResult<Vec<Skey>> {
    let key = TableKey::Index {
        id,
        cols,
        entity: None,
    };

    match table.get(&key)? {
        Some(guard) => match guard.value() {
            TableValue::Skey(entity) => Ok(vec![entity]),
            _ => Err(SdbError::Corrupt("unique index entry without an entity key".into())),
        },
        None => Ok(Vec::new()),
    }
}

/// Every entity a non-unique index stores for `cols` (each in its entry's key,
/// after the columns). Scans the `[cols·min_entity, cols·max_entity]` key range.
fn duplicate_matches(table: &ReadOnlyTable<TableKey, TableValue>, id: IndexId, cols: Vec<u8>) -> SdbResult<Vec<Skey>> {
    let low = TableKey::Index {
        id,
        cols: cols.clone(),
        entity: Some(Skey::from_bytes([0x00; 16])),
    };
    let high = TableKey::Index {
        id,
        cols,
        entity: Some(Skey::from_bytes([0xFF; 16])),
    };

    let mut entities = Vec::new();
    for item in table.range(low..=high)? {
        let (key, _) = item?;
        if let TableKey::Index {
            entity: Some(entity), ..
        } = key.value()
        {
            entities.push(entity);
        }
    }

    Ok(entities)
}
