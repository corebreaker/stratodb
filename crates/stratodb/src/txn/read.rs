//! Opaque read transaction.

use super::{query::IndexQuery, rooted::RootedRead};
use crate::{
    access::{ReadCursor, Reader, Rooted},
    cache::PathCache,
    data::{refs::SRef, SData, SValue, Scalar},
    engine::{self, TableKey, TableValue},
    error::{SdbError, SdbResult},
    index::{registry, IndexId, Pattern},
    node::{Node, NodeKind},
    path::{IntoPath, SPath, Segment},
    tree,
    Skey,
};

use redb::{ReadOnlyTable, ReadTransaction, TableError};
use std::{collections::HashSet, sync::Arc};

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
    pub(super) fn open(&self) -> SdbResult<Option<ReadOnlyTable<TableKey, TableValue>>> {
        match self.txn.open_table(engine::data_def(&self.table)) {
            Ok(table) => Ok(Some(table)),
            Err(TableError::TableDoesNotExist(_)) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Reads the value at `path`, decoded as `V`.
    pub fn get<V: SValue>(&self, path: impl IntoPath) -> SdbResult<Option<V>> {
        self.get_at(&path.into_path()?)
    }

    /// Reads the raw scalar at `path`.
    pub fn get_scalar(&self, path: impl IntoPath) -> SdbResult<Option<Scalar>> {
        self.scalar_at_path(&path.into_path()?)
    }

    /// Reports the kind of node at `path`, if any.
    pub fn kind(&self, path: impl IntoPath) -> SdbResult<Option<NodeKind>> {
        self.kind_at(&path.into_path()?)
    }

    /// Returns whether a node exists at `path`.
    pub fn exists(&self, path: impl IntoPath) -> SdbResult<bool> {
        Ok(self.kind(path)?.is_some())
    }

    /// Reads a typed read accessor for the value at `path`.
    pub fn fetch<'t, A: SRef<'t>>(&'t self, path: impl IntoPath) -> SdbResult<A> {
        self.fetch_at(&path.into_path()?)
    }

    /// Recomposes a whole `T` from the subtree at `path`.
    pub fn load<T: SData>(&self, path: impl IntoPath) -> SdbResult<T> {
        self.load_at(&path.into_path()?)
    }

    /// Returns a view of this transaction whose paths are relative to `root`.
    ///
    /// Every path passed to the returned [`RootedRead`] resolves as `root` then
    /// the path, so `txn.rooted("users/alice")?.get("age")` reads
    /// `users/alice/age`. The view borrows the transaction. An index query on the
    /// view is scoped to entities at or under `root` (see [`RootedRead::find`]).
    pub fn rooted(&self, root: impl IntoPath) -> SdbResult<RootedRead<'_>> {
        Ok(RootedRead::new(self, root.into_path()?))
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

    /// Finds the entities an index points at, recomposing each as a `T`.
    ///
    /// `values` are matched against the index's leading columns (in column order):
    /// the full set is an exact lookup, fewer is a prefix lookup (every entity
    /// whose leading columns match), and an empty slice matches every indexed
    /// entity. Results come back in index order (ascending by the encoded key,
    /// honoring each column's ASC/DESC). For reverse order, prefix bounds, or a
    /// subtree scope, use [`query`](Self::query). Errors with
    /// [`SdbError::IndexNotFound`] for an unknown index and [`SdbError::IndexArity`]
    /// when given more values than the index has columns.
    pub fn find<T: SData>(&self, index: &str, values: &[Scalar]) -> SdbResult<Vec<T>> {
        self.query(index).prefixed(values).run()
    }

    /// Starts an [`IndexQuery`] against `index` — a builder for prefix matches,
    /// reverse order, and subtree scoping. See [`find`](Self::find) for the common
    /// exact/forward case.
    pub fn query(&self, index: &str) -> IndexQuery<'_> {
        IndexQuery::new(self, index)
    }

    /// Runs a built index query: prefix scan (exact = full prefix), optional
    /// subtree scoping, optional reversal, then recomposes each hit as a `T`.
    pub(crate) fn execute_query<T: SData>(
        &self,
        index: &str,
        prefix: &[Scalar],
        reverse: bool,
        root: &SPath,
    ) -> SdbResult<Vec<T>> {
        let entry = {
            let meta = self.txn.open_table(engine::META_TABLE)?;

            registry::lookup(&meta, &self.table, index)?
        }
        .ok_or_else(|| SdbError::IndexNotFound {
            index: index.to_string(),
        })?;

        let def = entry.def();
        if prefix.len() > def.columns().len() {
            return Err(SdbError::IndexArity {
                index:    index.to_string(),
                expected: def.columns().len(),
                got:      prefix.len(),
            });
        }

        let id = entry.id();

        // `encode_columns` zips with the columns, so a short `prefix` encodes only
        // its leading columns — exactly the byte prefix a prefix scan needs.
        let cols = def.encode_columns(prefix);

        let Some(table) = self.open()? else {
            return Ok(Vec::new());
        };

        let mut entities = scan_prefix(&table, id, &cols, def.unique())?;

        // Restrict to entities at or under `root`. An entity is in scope only if
        // the pattern reaches at least the root's depth; when it does,
        // `affected_entities(root)` yields exactly the matching entities on (and
        // therefore under) that root.
        if !root.is_empty() {
            let pattern = Pattern::parse(def.pattern())?;
            if pattern.depth() < root.len() {
                return Ok(Vec::new());
            }

            let under: HashSet<Skey> = pattern.affected_entities(&table, root)?.into_iter().collect();
            entities.retain(|entity| under.contains(entity));
        }

        // The scan yields ascending index order; reverse the materialized hits for
        // descending order (the whole result set is loaded either way).
        if reverse {
            entities.reverse();
        }

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

/// Collects every entity whose index entry's columns start with `prefix`, in
/// ascending key order.
///
/// Seeks to the first entry for `id` at or after `prefix`, then walks forward
/// until an entry no longer matches (different index, or columns diverging from
/// `prefix`) and stops — so an exact lookup is just the full-length prefix. The
/// matched entity lives in the key for a non-unique index and in the value for a
/// unique one; the seek's lower bound is tagged accordingly.
fn scan_prefix(
    table: &ReadOnlyTable<TableKey, TableValue>,
    id: IndexId,
    prefix: &[u8],
    unique: bool,
) -> SdbResult<Vec<Skey>> {
    let lower = TableKey::Index {
        id,
        cols: prefix.to_vec(),
        entity: (!unique).then(|| Skey::from_bytes([0x00; 16])),
    };

    let mut entities = Vec::new();
    for item in table.range(lower..)? {
        let (key, value) = item?;
        match key.value() {
            TableKey::Index {
                id: entry_id,
                cols,
                entity,
            } if entry_id == id && cols.starts_with(prefix) => {
                let entity = match entity {
                    Some(entity) => entity,
                    None => match value.value() {
                        TableValue::Skey(entity) => entity,
                        _ => return Err(SdbError::Corrupt("unique index entry without an entity key".into())),
                    },
                };

                entities.push(entity);
            }
            _ => break,
        }
    }

    Ok(entities)
}
