//! Opaque write transaction.

use super::rooted::RootedWrite;
use crate::{
    access::{BoundCursor, MemReader, Reader, WriteCursor, Writer},
    data::{refs::SMut, SData, Scalar, SValue},
    db::DbInner,
    engine::{self, ArchivedNodes, MemNodes, TableKey, TableValue},
    error::{SdbError, SdbResult},
    index::{
        maintenance,
        registry::{self, IndexEntry},
        Pattern,
    },
    node::{Node, NodeKind},
    path::{IntoPath, SPath, Segment},
    tree::{self, Located},
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

        if self.should_pack(base)? {
            // Pack: build the whole entity subtree in memory, then write it as a
            // single engine value. The decomposition is the very same `SData::store`
            // logic, just driven over an in-memory mini node-table.
            let mut mem = MemNodes::new();
            {
                let cell = RefCell::new(&mut mem);
                value.store(&BoundCursor::new(&cell, Skey::ROOT), &SPath::root())?;
            }

            let node = mem.into_packed()?;
            let last = last.clone();

            return self.reindex_around(base, move |table| {
                tree::remove_path(table, base)?;
                tree::store_packed(table, &parent_path, &last, node)
            });
        }

        // Unpacked (an index reaches into this subtree, so its children need their
        // own keys): one index-maintenance bracket and one table handle for the
        // whole entity — clear the old subtree, resolve the entity's parent once,
        // then let every field write resolve relative to that anchor.
        let rel = segment_path(last);
        self.reindex_around(base, |table| {
            tree::remove_path(table, base)?;
            let anchor = tree::ensure_container(table, &parent_path, matches!(last, Segment::Index(_)))?;

            let cell = RefCell::new(table);
            value.store(&BoundCursor::new(&cell, anchor), &rel)
        })
    }

    /// Whether the subtree stored at `base` may be packed into one engine value:
    /// it may, unless some index pattern matches entities *strictly below* `base`
    /// (those need their own keys, so the subtree must stay shredded).
    pub(crate) fn should_pack(&self, base: &SPath) -> SdbResult<bool> {
        let patterns = self.index_patterns()?;

        Ok(!patterns.iter().any(|p| p.covers_strictly_below(base)))
    }

    /// This table's index patterns, parsed once.
    fn index_patterns(&self) -> SdbResult<Vec<Pattern>> {
        self.indexes()?
            .iter()
            .map(|entry| Pattern::parse(entry.def().pattern()))
            .collect()
    }

    /// Stores many `(path, value)` pairs in this transaction.
    ///
    /// With the `parallel` feature, the CPU-bound packing of each entity (building
    /// its blob) runs across rayon threads; the engine writes are then applied
    /// sequentially (a redb write transaction is single-threaded). Without the
    /// feature this is a plain sequential loop of [`store`](Self::store). Either
    /// way the result is exactly that of storing each pair in order.
    #[cfg(feature = "parallel")]
    pub fn store_many<T: SData + Sync>(&self, items: &[(SPath, &T)]) -> SdbResult<()> {
        use rayon::prelude::*;

        /// One pair's pre-built result: a ready packed node, or an index back into
        /// `items` for the (rare) pairs that cannot pack and fall back to `store`.
        enum Built {
            Packed {
                base:   SPath,
                parent: SPath,
                last:   Segment,
                node:   crate::node::Node,
            },
            Plain(usize),
        }

        // Patterns are read once, before the parallel section: the build closure
        // must not touch the (non-`Sync`) write transaction, only pure CPU work.
        let patterns = self.index_patterns()?;
        let packable = |base: &SPath| !patterns.iter().any(|p| p.covers_strictly_below(base));

        let built = items
            .par_iter()
            .enumerate()
            .map(|(index, (path, value))| match path.split_last() {
                Some((parent, last)) if packable(path) => {
                    let mut mem = MemNodes::new();
                    {
                        let cell = RefCell::new(&mut mem);
                        value.store(&BoundCursor::new(&cell, Skey::ROOT), &SPath::root())?;
                    }

                    Ok(Built::Packed {
                        base: path.clone(),
                        parent,
                        last: last.clone(),
                        node: mem.into_packed()?,
                    })
                }
                _ => Ok(Built::Plain(index)),
            })
            .collect::<SdbResult<Vec<_>>>()?;

        // Apply every packed write under one table handle (no per-entity reopen),
        // resolving each distinct parent only once and bracketing each with index
        // maintenance. Non-packable pairs are deferred and stored after the handle
        // is dropped (each opens its own).
        let mut plain = Vec::new();
        {
            let indexes = self.indexes()?;
            let mut table = self.txn.open_table(engine::data_def(&self.table))?;
            // Parent path -> key, valid for this loop: it only ever creates/links
            // children (additive), so a parent's key never changes mid-batch.
            let mut parents: std::collections::HashMap<SPath, Skey> = std::collections::HashMap::new();

            for item in built {
                match item {
                    Built::Packed {
                        base,
                        parent,
                        last,
                        node,
                    } => {
                        if !indexes.is_empty() {
                            maintenance::delete(&mut table, indexes, &base)?;
                        }

                        let parent_key = match parents.get(&parent) {
                            Some(key) => *key,
                            None => {
                                let key =
                                    tree::ensure_container(&mut table, &parent, matches!(last, Segment::Index(_)))?;
                                parents.insert(parent.clone(), key);
                                key
                            }
                        };

                        tree::store_packed_under(&mut table, parent_key, &parent, &last, node)?;

                        if !indexes.is_empty() {
                            maintenance::insert(&mut table, indexes, &base)?;
                        }
                    }
                    Built::Plain(index) => plain.push(index),
                }
            }
        }

        for index in plain {
            self.store_at(&items[index].0, items[index].1)?;
        }

        Ok(())
    }

    /// Sequential `store_many` when the `parallel` feature is off.
    #[cfg(not(feature = "parallel"))]
    pub fn store_many<T: SData>(&self, items: &[(SPath, &T)]) -> SdbResult<()> {
        for (path, value) in items {
            self.store_at(path, *value)?;
        }

        Ok(())
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
        // A write accessor navigates and mutates by node key, which a packed entity
        // has only inside its blob. Spill the enclosing entity back into the live
        // table first, so the accessor works against ordinary shredded nodes.
        self.unpack_if_packed(base)?;

        let cursor = WriteCursor::new(self);
        let key = cursor
            .resolve(base)?
            .ok_or_else(|| SdbError::PathNotFound(base.clone()))?;

        Ok(A::open(Arc::new(cursor), base.clone(), key))
    }

    /// If `base` lands at or inside a packed entity, unpacks that entity in place
    /// so subsequent key-addressed access sees plain nodes.
    fn unpack_if_packed(&self, base: &SPath) -> SdbResult<()> {
        let mut table = self.txn.open_table(engine::data_def(&self.table))?;
        if let Located::Packed {
            entity, ..
        } = tree::locate(&table, base)?
        {
            tree::unpack_entity(&mut table, entity)?;
        }

        Ok(())
    }

    pub(crate) fn load_at<T: SData>(&self, base: &SPath) -> SdbResult<T> {
        // If `base` is at/inside a packed entity, decode it and recompose from the
        // blob. The table handle is dropped before any `WriteCursor` use, since a
        // write transaction allows only one open handle on the table at a time.
        let packed = {
            let table = self.txn.open_table(engine::data_def(&self.table))?;
            match tree::locate(&table, base)? {
                Located::Packed {
                    entity,
                    rel,
                } => {
                    // A write transaction sees its own uncommitted blob; read it
                    // archived (zero-copy) just like a committed read, without the
                    // shared cache (which serves committed snapshots only).
                    let arch = match tree::read_node(&table, entity)? {
                        Some(Node::Packed {
                            blob, ..
                        }) => ArchivedNodes::new(&blob)?,
                        _ => {
                            return Err(SdbError::Corrupt(
                                "locate reported a packed entity that is not packed".into(),
                            ));
                        }
                    };

                    let root = tree::resolve_from(&arch, Skey::ROOT, &rel)?;

                    Some((arch, root))
                }
                _ => None,
            }
        };

        match packed {
            Some((arch, Some(root))) => T::load(&MemReader::new(Arc::new(arch), root, SPath::root()), &SPath::root()),
            // Plain, absent, or an absent sub-path of a packed entity: the path
            // loader handles all three (including a field's `default` fallback).
            _ => T::load(&WriteCursor::new(self), base),
        }
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
        self.reindex_around(path, |table| match tree::locate(table, path)? {
            // The path descends into a packed entity: read-modify-write its blob.
            Located::Packed {
                entity,
                rel,
            } => {
                let mut mem = tree::decode_packed(table, entity)?;
                tree::put_scalar_rel(&mut mem, Skey::ROOT, &rel, scalar)?;

                tree::write_packed(table, entity, mem.into_packed()?)
            }
            _ => tree::put_scalar(table, path, scalar),
        })
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
        self.reindex_around(path, |table| match tree::locate(table, path)? {
            // Removing *inside* a packed entity edits its blob; removing the entity
            // itself (empty `rel`) falls through to the plain single-entry delete.
            Located::Packed {
                entity,
                rel,
            } if !rel.is_empty() => {
                let mut mem = tree::decode_packed(table, entity)?;
                let removed = tree::remove_rel(&mut mem, Skey::ROOT, &rel)?;
                if removed {
                    tree::write_packed(table, entity, mem.into_packed()?)?;
                }

                Ok(removed)
            }
            _ => tree::remove_path(table, path),
        })
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
