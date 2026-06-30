//! Write-time index maintenance.
//!
//! A mutation keeps every index in sync by re-indexing the entities it could
//! have affected — those the mutation's path passes through (see
//! [`Pattern::affected_entities`]). The caller brackets the mutation: [`delete`]
//! the affected entities' current entries *before* the change, apply the change,
//! then [`insert`] the entries the new state implies. Because an entry's key is a
//! pure function of the entity's column values and key, this delete-then-insert
//! cleanly covers every case — creation, replacement, column edits and removal —
//! without diffing.
//!
//! Each [`encode_scalar`](super::ordered::encode_scalar) of a missing or non-leaf
//! column yields `Null`, so an entity always produces a well-formed key.

use super::{definitions::IndexDef, pattern::Pattern, registry::IndexEntry, IndexId};
use crate::{
    data::Scalar,
    engine::{TableKey, TableValue},
    error::{SdbError, SdbResult},
    path::SPath,
    tree,
    Skey,
};

use redb::{ReadableTable, Table};

/// The writable engine table holding both data nodes and index entries.
type DataTable<'txn> = Table<'txn, TableKey, TableValue>;

/// Removes the index entries the affected entities currently have. Call before
/// applying a mutation at `scope`.
pub(crate) fn delete(data: &mut DataTable<'_>, indexes: &[IndexEntry], scope: &SPath) -> SdbResult<()> {
    for entry in indexes {
        for (key, _) in entries_for(&*data, entry, scope)? {
            data.remove(&key)?;
        }
    }

    Ok(())
}

/// Removes every physical entry belonging to `entry`, across all indexed
/// entities. Call when an index is dropped: its registry record is gone, so its
/// entries in the data table must go too.
///
/// An index's entries occupy one contiguous key block — the same leading
/// `tag · id`, the `unique` flag fixing the tag — so a single forward range scan
/// from that id's lower bound, stopping at the first entry with a different id,
/// covers exactly this index and nothing else (ids are never reused, so no other
/// index shares one).
pub(crate) fn delete_all(data: &mut DataTable<'_>, entry: &IndexEntry) -> SdbResult<()> {
    let id = entry.id();
    let lower = TableKey::Index {
        id,
        cols: Vec::new(),
        entity: (!entry.def().unique()).then(|| Skey::from_bytes([0x00; 16])),
    };

    let mut keys = Vec::new();
    for item in data.range(lower..)? {
        let (key, _) = item?;
        let table_key = key.value();

        if !matches!(&table_key, TableKey::Index { id: entry_id, .. } if *entry_id == id) {
            break;
        }

        keys.push(table_key);
    }

    for key in keys {
        data.remove(&key)?;
    }

    Ok(())
}

/// Inserts the index entries the affected entities now imply. Call after applying
/// a mutation at `scope`. A unique index rejects an entry whose key already maps
/// to a different entity with [`SdbError::UniqueViolation`].
pub(crate) fn insert(data: &mut DataTable<'_>, indexes: &[IndexEntry], scope: &SPath) -> SdbResult<()> {
    for entry in indexes {
        let unique = entry.def().unique();
        for (key, value) in entries_for(&*data, entry, scope)? {
            if unique {
                guard_unique(data, &key, &value, entry.def().name())?;
            }

            data.insert(&key, &value)?;
        }
    }

    Ok(())
}

/// Fails with [`SdbError::UniqueViolation`] if `key` already maps to an entity
/// other than the one in `value`. Re-inserting an entity's own entry is fine —
/// `delete` removes it first — so this fires only on a genuine collision (a
/// second entity, or a batch with a repeated value).
fn guard_unique(data: &DataTable<'_>, key: &TableKey, value: &TableValue, index: &str) -> SdbResult<()> {
    let TableValue::Skey(entity) = value else {
        return Ok(());
    };

    if let Some(existing) = data.get(key)?
        && matches!(existing.value(), TableValue::Skey(other) if other != *entity)
    {
        return Err(SdbError::UniqueViolation {
            index: index.to_string(),
        });
    }

    Ok(())
}

/// The index entries for every entity `entry` matches on `scope`'s line.
fn entries_for<T: ReadableTable<TableKey, TableValue>>(
    t: &T,
    entry: &IndexEntry,
    scope: &SPath,
) -> SdbResult<Vec<(TableKey, TableValue)>> {
    let pattern = Pattern::parse(entry.def().pattern())?;

    pattern
        .affected_entities(t, scope)?
        .into_iter()
        .map(|entity| index_key(t, entry.id(), entry.def(), entity))
        .collect()
}

/// Builds the key/value an index entry for `entity` takes: a non-unique index
/// puts the entity key in the table key (a tie-breaker after the columns); a
/// unique index puts it in the value, so the columns alone form the key.
fn index_key<T: ReadableTable<TableKey, TableValue>>(
    t: &T,
    id: IndexId,
    def: &IndexDef,
    entity: Skey,
) -> SdbResult<(TableKey, TableValue)> {
    let values = def
        .columns()
        .iter()
        .map(|column| column_scalar(t, entity, column.path()))
        .collect::<SdbResult<Vec<_>>>()?;
    let cols = def.encode_columns(&values);

    let table_key = TableKey::Index {
        id,
        cols,
        entity: (!def.unique()).then_some(entity),
    };
    let value = if def.unique() {
        TableValue::Skey(entity)
    } else {
        TableValue::Unit
    };

    Ok((table_key, value))
}

/// The scalar at `column` relative to `entity`, or `Null` when the path is absent
/// or does not land on a leaf. Descends transparently into a packed entity's blob.
fn column_scalar<T: ReadableTable<TableKey, TableValue>>(t: &T, entity: Skey, column: &SPath) -> SdbResult<Scalar> {
    Ok(tree::entity_leaf(t, entity, column)?.unwrap_or(Scalar::Null))
}
