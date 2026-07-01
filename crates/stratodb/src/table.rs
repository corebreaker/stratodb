//! A handle to a single table, from which transactions are started.

use crate::{
    cache::PathCache,
    db::DbInner,
    engine::{self, META_TABLE},
    error::{SdbError, SdbResult},
    index::{maintenance, registry, IndexDef, SIndexed},
    path::SPath,
    txn::{ReadTxn, WriteTxn},
};

use redb::ReadableDatabase;
use std::sync::{atomic::Ordering, Arc};

/// A lightweight handle to a named table.
///
/// Cloning is cheap. Reads run concurrently; writes are serialized (a single
/// writer at a time), matching the underlying engine's transaction model. The
/// handle carries a shared `PathCache` so a transaction's accessors reuse
/// resolved paths.
#[derive(Clone)]
pub struct Table {
    inner: Arc<DbInner>,
    name:  String,
    cache: Arc<PathCache>,
}

impl Table {
    pub(crate) fn new(inner: Arc<DbInner>, name: String, cache: Arc<PathCache>) -> Self {
        Self {
            inner,
            name,
            cache,
        }
    }

    /// The table's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Begins a read transaction. Multiple read transactions may run concurrently
    /// with each other and with a single writer.
    pub fn read(&self) -> SdbResult<ReadTxn> {
        // Capture the snapshot and its matching generation atomically with respect
        // to commits, so cached resolutions are never attributed to the wrong one.
        let guard = self
            .inner
            .version_lock()
            .read()
            .map_err(|err| SdbError::CannotAccess(format!("version lock poisoned: {err}")))?;

        let txn = self.inner.db().begin_read()?;
        let generation = self.inner.generation().load(Ordering::Acquire);
        drop(guard);

        Ok(ReadTxn::new(
            txn,
            self.name.clone(),
            generation,
            Arc::clone(&self.cache),
        ))
    }

    /// Begins a write transaction. Only one write transaction may be active at a
    /// time across the whole database.
    pub fn write(&self) -> SdbResult<WriteTxn> {
        let txn = self.inner.db().begin_write()?;

        Ok(WriteTxn::new(txn, self.name.clone(), Arc::clone(&self.inner)))
    }

    /// Registers a secondary index on this table and back-fills it.
    ///
    /// Idempotent for an identical definition; errors with
    /// [`SchemaMismatch`](crate::SdbError::SchemaMismatch) if `def.name` already
    /// names a different index here. On first creation, every pre-existing entity
    /// the index matches is indexed too, so the index is correct whether data was
    /// written before or after — and creating a unique index over duplicate data
    /// fails with [`UniqueViolation`](crate::SdbError::UniqueViolation).
    pub fn create_index(&self, def: &IndexDef) -> SdbResult<()> {
        let txn = self.inner.db().begin_write()?;

        // Register; on a fresh creation, recover the entry (it carries the new id)
        // so the back-fill below can build its keys.
        let new_entry = {
            let mut meta = txn.open_table(META_TABLE)?;
            if registry::create(&mut meta, &self.name, def)? {
                registry::lookup(&meta, &self.name, def.name())?
            } else {
                None
            }
        };

        // Back-fill: index every entity the new index already matches.
        if let Some(entry) = new_entry {
            let mut data = txn.open_table(engine::data_def(&self.name))?;
            maintenance::insert(&mut data, std::slice::from_ref(&entry), &SPath::root())?;
        }

        txn.commit()?;
        self.inner.bump_schema_gen();

        Ok(())
    }

    /// Registers every index that `T` declares (via `#[strato(index(...))]`) on
    /// this table, scoping each to `pattern`, and back-fills them.
    ///
    /// `pattern` selects which nodes are the indexed *entities*: a slash-separated
    /// path where `*` matches any single child and every other segment matches
    /// literally (a list index such as `items[0]` is allowed). For example
    /// `"users/*"` indexes every direct child of `users`, so each declared column
    /// (a path relative to a matched entity) resolves under `users/<id>/…`. It is
    /// not recursive — `*` is one level — and the empty string `""` scopes the
    /// index to the table root. See [`SIndexed::index_defs`] for the full model.
    ///
    /// A shorthand for calling [`create_index`](Self::create_index) for each of
    /// [`T::index_defs`](SIndexed::index_defs); each is idempotent and back-filled.
    pub fn create_indexes<T: SIndexed>(&self, pattern: &str) -> SdbResult<()> {
        for def in T::index_defs(pattern) {
            self.create_index(&def)?;
        }

        Ok(())
    }

    /// Registers `def` and back-fills it **only if no index of that name already
    /// exists** on this table; otherwise the call is a no-op and the existing
    /// index is left exactly as it is — its definition is *not* reconciled with
    /// `def`.
    ///
    /// The idempotent-by-name counterpart of [`create_index`](Self::create_index):
    /// where `create_index` errors with
    /// [`SchemaMismatch`](crate::SdbError::SchemaMismatch) on a name clash with a
    /// divergent definition, `ensure_index` simply leaves the present index in
    /// place. When it does create the index, the back-fill (and unique-violation
    /// check) behaves exactly like `create_index`.
    pub fn ensure_index(&self, def: &IndexDef) -> SdbResult<()> {
        let txn = self.inner.db().begin_write()?;

        // Present by name (whatever its definition) -> leave it untouched. Else
        // register and recover the new entry (it carries the id) for the back-fill.
        let new_entry = {
            let mut meta = txn.open_table(META_TABLE)?;
            if registry::has(&meta, &self.name, def.name())? {
                None
            } else {
                registry::create(&mut meta, &self.name, def)?;
                registry::lookup(&meta, &self.name, def.name())?
            }
        };

        if let Some(entry) = new_entry {
            let mut data = txn.open_table(engine::data_def(&self.name))?;
            maintenance::insert(&mut data, std::slice::from_ref(&entry), &SPath::root())?;
        }

        txn.commit()?;
        self.inner.bump_schema_gen();

        Ok(())
    }

    /// Ensures every index that `T` declares (via `#[strato(index(...))]`) exists
    /// on this table, scoping each to `pattern`. The idempotent-by-name
    /// counterpart of [`create_indexes`](Self::create_indexes): a shorthand for
    /// [`ensure_index`](Self::ensure_index) on each of
    /// [`T::index_defs`](SIndexed::index_defs), so a declared index is created and
    /// back-filled when absent and left untouched when one of that name already
    /// exists. See [`create_indexes`](Self::create_indexes) for the `pattern`
    /// meaning.
    pub fn ensure_indexes<T: SIndexed>(&self, pattern: &str) -> SdbResult<()> {
        for def in T::index_defs(pattern) {
            self.ensure_index(&def)?;
        }

        Ok(())
    }

    /// Returns the definition of the named index on this table, if it exists.
    pub fn index_def(&self, name: &str) -> SdbResult<Option<IndexDef>> {
        let txn = self.inner.db().begin_read()?;
        let meta = txn.open_table(META_TABLE)?;

        Ok(registry::lookup(&meta, &self.name, name)?.map(|entry| entry.into_def()))
    }

    /// Returns whether an index named `name` is registered on this table.
    ///
    /// Optimized for presence alone: it scans the registry and stops at the first
    /// match, decoding only each record's table and index name — never a full
    /// [`IndexDef`] (no column path is parsed). Use [`index_def`](Self::index_def)
    /// when the definition itself is needed.
    pub fn has_index(&self, name: &str) -> SdbResult<bool> {
        let txn = self.inner.db().begin_read()?;
        let meta = txn.open_table(META_TABLE)?;

        registry::has(&meta, &self.name, name)
    }

    /// Drops the index named `name` from this table: in one atomic transaction it
    /// removes the registry record and purges every entry the index holds in the
    /// data table.
    ///
    /// Returns whether an index was actually removed — `false` (idempotent) when
    /// no index of that name exists here.
    pub fn delete_index(&self, name: &str) -> SdbResult<bool> {
        let txn = self.inner.db().begin_write()?;

        // Drop the registry record first; the recovered entry carries the id and
        // uniqueness the physical purge below needs.
        let removed = {
            let mut meta = txn.open_table(META_TABLE)?;
            registry::delete(&mut meta, &self.name, name)?
        };

        let found = removed.is_some();
        if let Some(entry) = removed {
            let mut data = txn.open_table(engine::data_def(&self.name))?;
            maintenance::delete_all(&mut data, &entry)?;
        }

        txn.commit()?;
        self.inner.bump_schema_gen();

        Ok(found)
    }

    /// Drops every index that `T` declares (via `#[strato(index(...))]`) from this
    /// table, returning how many were actually removed.
    ///
    /// The mirror of [`create_indexes`](Self::create_indexes): an index's name is
    /// fixed by the derive, independent of any scope, so no pattern is needed.
    /// Each drop is idempotent — an index `T` declares but that was never created
    /// here is simply skipped (and not counted).
    pub fn delete_indexes<T: SIndexed>(&self) -> SdbResult<usize> {
        // The pattern only fills `IndexDef::pattern`, which a drop ignores; the
        // names — all we use — are independent of it.
        let mut removed = 0;
        for def in T::index_defs("") {
            if self.delete_index(def.name())? {
                removed += 1;
            }
        }

        Ok(removed)
    }
}
