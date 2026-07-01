//! Opaque read transaction.

use super::{query::IndexQuery, rooted::RootedRead};
use crate::{
    access::{MemReader, ReadCursor, Reader, Rooted},
    cache::PathCache,
    data::{refs::SRef, SData, SValue, Scalar},
    engine::{self, ArchivedNodes, TableKey, TableValue},
    error::{SdbError, SdbResult},
    index::{registry, IndexId, Pattern},
    node::{Node, NodeKind},
    path::{IntoPath, SPath, Segment},
    tree::{self, Located},
    Skey,
};

use redb::{ReadOnlyTable, ReadTransaction, TableError};
use std::{collections::HashSet, sync::Arc, sync::OnceLock};

/// A read-only view of a table at a consistent point in time.
pub struct ReadTxn {
    txn:        ReadTransaction,
    table:      String,
    generation: u64,
    cache:      Arc<PathCache>,
    /// The opened data table, materialized once and reused for every node read in
    /// this transaction (redb's `ReadOnlyTable` is independent of the transaction
    /// borrow). Reopening per read was a measurable cost on multi-field loads and
    /// index scans. `None` once computed means the table has never been written.
    data_table: OnceLock<Option<ReadOnlyTable<TableKey, TableValue>>>,
}

impl ReadTxn {
    pub(crate) fn new(txn: ReadTransaction, table: String, generation: u64, cache: Arc<PathCache>) -> Self {
        Self {
            txn,
            table,
            generation,
            cache,
            data_table: OnceLock::new(),
        }
    }

    /// The opened data table, cached for the lifetime of the transaction; `None` if
    /// it has never been written.
    pub(crate) fn open(&self) -> SdbResult<Option<&ReadOnlyTable<TableKey, TableValue>>> {
        if let Some(cached) = self.data_table.get() {
            return Ok(cached.as_ref());
        }

        let opened = match self.txn.open_table(engine::data_def(&self.table)) {
            Ok(table) => Some(table),
            Err(TableError::TableDoesNotExist(_)) => None,
            Err(error) => return Err(error.into()),
        };

        Ok(self.data_table.get_or_init(|| opened).as_ref())
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
        match self.open()? {
            Some(table) => tree::kind(table, path),
            None => Ok(None),
        }
    }

    pub(crate) fn fetch_at<'t, A: SRef<'t>>(&'t self, base: &SPath) -> SdbResult<A> {
        let Some(table) = self.open()? else {
            return Err(SdbError::PathNotFound(base.clone()));
        };

        match tree::locate(table, base)? {
            Located::Missing => Err(SdbError::PathNotFound(base.clone())),
            Located::Plain(_) => {
                let cursor = ReadCursor::new(self);
                let key = cursor
                    .resolve(base)?
                    .ok_or_else(|| SdbError::PathNotFound(base.clone()))?;

                Ok(A::open(Arc::new(cursor), base.clone(), key))
            }
            Located::Packed {
                entity,
                rel,
            } => {
                let mem = self
                    .packed_mem(table, entity)?
                    .ok_or_else(|| SdbError::Corrupt("locate reported a packed entity that is not packed".into()))?;
                let root =
                    tree::resolve_from(&*mem, Skey::ROOT, &rel)?.ok_or_else(|| SdbError::PathNotFound(base.clone()))?;

                Ok(A::open(
                    Arc::new(MemReader::new(mem, root, base.clone())),
                    base.clone(),
                    root,
                ))
            }
        }
    }

    pub(crate) fn load_at<T: SData>(&self, base: &SPath) -> SdbResult<T> {
        // Fast path: a cached resolution of `base` to a node key. This covers the
        // common whole-entity load — `base` lands on a node (plain or a packed
        // entity) rather than descending into one — and reuses the shared path
        // cache instead of re-walking the tree on every load. `lookup_path` only
        // opens the engine table on a cache miss, so a fully warm read (path and
        // blob both cached) serves the whole load from memory — the table is never
        // opened, an open redb pays on every read but StratoDB can skip.
        if let Some(key) = self.lookup_path(base)? {
            if let Some(mem) = self.cache.get_blob(self.generation, key)? {
                return T::load(&MemReader::new(mem, Skey::ROOT, SPath::root()), &SPath::root());
            }

            // Path cached but blob not (or `base` is a plain node): the table is
            // needed to read — and cache — the entity.
            let Some(table) = self.open()? else {
                return Err(SdbError::PathNotFound(base.clone()));
            };

            return match self.packed_mem(table, key)? {
                Some(mem) => T::load(&MemReader::new(mem, Skey::ROOT, SPath::root()), &SPath::root()),
                None => T::load(&ReadCursor::new(self), base),
            };
        }

        // `base` did not resolve directly: it is either inside a packed entity (the
        // walk stops at the entity) or genuinely absent.
        let Some(table) = self.open()? else {
            return Err(SdbError::PathNotFound(base.clone()));
        };

        match tree::locate(table, base)? {
            // Absent: the path loader applies any field `default` fallback.
            Located::Missing | Located::Plain(_) => T::load(&ReadCursor::new(self), base),
            Located::Packed {
                entity,
                rel,
            } => {
                let mem = self
                    .packed_mem(table, entity)?
                    .ok_or_else(|| SdbError::Corrupt("locate reported a packed entity that is not packed".into()))?;

                match tree::resolve_from(&*mem, Skey::ROOT, &rel)? {
                    Some(root) => T::load(&MemReader::new(mem, root, SPath::root()), &SPath::root()),
                    None => T::load(&ReadCursor::new(self), base),
                }
            }
        }
    }

    /// The decoded blob of the packed entity at `key`, served from (and populated
    /// into) the shared blob cache, or `None` if `key` is not a packed entity.
    fn packed_mem(
        &self,
        table: &ReadOnlyTable<TableKey, TableValue>,
        key: Skey,
    ) -> SdbResult<Option<Arc<ArchivedNodes>>> {
        if let Some(mem) = self.cache.get_blob(self.generation, key)? {
            return Ok(Some(mem));
        }

        match tree::read_node(table, key)? {
            Some(Node::Packed {
                blob, ..
            }) => {
                let mem = Arc::new(ArchivedNodes::new(&blob)?);
                self.cache.put_blob(self.generation, key, Arc::clone(&mem))?;

                Ok(Some(mem))
            }
            _ => Ok(None),
        }
    }

    /// Recomposes the entity stored under `entity` as a `T`, decoding it (via the
    /// blob cache) when it is packed and otherwise re-rooting a plain reader at it.
    fn load_entity<T: SData>(&self, table: &ReadOnlyTable<TableKey, TableValue>, entity: Skey) -> SdbResult<T> {
        match self.packed_mem(table, entity)? {
            Some(mem) => T::load(&MemReader::new(mem, Skey::ROOT, SPath::root()), &SPath::root()),
            None => T::load(&Rooted::new(&ReadCursor::new(self), entity), &SPath::root()),
        }
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

        let mut entities = scan_prefix(table, id, &cols, def.unique())?;

        // Restrict to entities at or under `root`. An entity is in scope only if
        // the pattern reaches at least the root's depth; when it does,
        // `affected_entities(root)` yields exactly the matching entities on (and
        // therefore under) that root.
        if !root.is_empty() {
            let pattern = Pattern::parse(def.pattern())?;
            if pattern.depth() < root.len() {
                return Ok(Vec::new());
            }

            let under: HashSet<Skey> = pattern.affected_entities(table, root)?.into_iter().collect();
            entities.retain(|entity| under.contains(entity));
        }

        // The scan yields ascending index order; reverse the materialized hits for
        // descending order (the whole result set is loaded either way).
        if reverse {
            entities.reverse();
        }

        // Each match is addressed by its (stable) key; recompose it from its own
        // subtree, decoding the blob in place when the entity is packed.
        entities
            .into_iter()
            .map(|entity| self.load_entity::<T>(table, entity))
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

        let resolved = tree::resolve(table, path)?;
        if let Some(key) = resolved {
            self.cache.put(self.generation, path, key)?;
        }

        Ok(resolved)
    }

    /// Reads the scalar at `path`; errors if the node there is not a leaf.
    ///
    /// A field inside a packed entity is served through the shared blob cache and
    /// navigated zero-copy, so reading one field of a hot entity reuses its already
    /// decoded archive instead of re-decoding the whole blob — the win over a flat
    /// value store, which must decode the entire record to reach any single field.
    fn scalar_at_path(&self, path: &SPath) -> SdbResult<Option<Scalar>> {
        let Some(table) = self.open()? else {
            return Ok(None);
        };

        match tree::locate(table, path)? {
            tree::Located::Missing => Ok(None),
            tree::Located::Plain(key) => tree::leaf_at(table, key, path),
            tree::Located::Packed {
                entity,
                rel,
            } => {
                let mem = self
                    .packed_mem(table, entity)?
                    .ok_or_else(|| SdbError::Corrupt("locate reported a packed entity that is not packed".into()))?;

                match tree::resolve_from(&*mem, Skey::ROOT, &rel)? {
                    Some(key) => tree::leaf_at(&*mem, key, path),
                    None => Ok(None),
                }
            }
        }
    }

    pub(crate) fn lookup_child(&self, parent: Skey, seg: &Segment) -> SdbResult<Option<Skey>> {
        let Some(table) = self.open()? else {
            return Ok(None);
        };

        tree::child_key(table, parent, seg)
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

        let resolved = tree::child_key(table, parent, seg)?;
        if let Some(key) = resolved {
            self.cache.put(self.generation, child_path, key)?;
        }

        Ok(resolved)
    }

    pub(crate) fn lookup_scalar(&self, key: Skey) -> SdbResult<Scalar> {
        let table = self
            .open()?
            .ok_or_else(|| SdbError::Corrupt("read on a missing table".into()))?;

        tree::scalar_at(table, key)
    }

    pub(crate) fn lookup_scalar_at(&self, path: &SPath) -> SdbResult<Option<Scalar>> {
        self.scalar_at_path(path)
    }

    pub(crate) fn lookup_kind(&self, key: Skey) -> SdbResult<Option<NodeKind>> {
        let Some(table) = self.open()? else {
            return Ok(None);
        };

        tree::kind_of(table, key)
    }

    pub(crate) fn lookup_len(&self, key: Skey) -> SdbResult<usize> {
        let table = self
            .open()?
            .ok_or_else(|| SdbError::Corrupt("read on a missing table".into()))?;

        tree::list_len(table, key)
    }

    pub(crate) fn lookup_object_keys(&self, key: Skey) -> SdbResult<Vec<String>> {
        let table = self
            .open()?
            .ok_or_else(|| SdbError::Corrupt("read on a missing table".into()))?;

        tree::object_keys(table, key)
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
                    None => match value.value().into_owned() {
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
