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
    error::SdbResult,
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

/// Inserts the index entries the affected entities now imply. Call after applying
/// a mutation at `scope`.
pub(crate) fn insert(data: &mut DataTable<'_>, indexes: &[IndexEntry], scope: &SPath) -> SdbResult<()> {
    for entry in indexes {
        for (key, value) in entries_for(&*data, entry, scope)? {
            data.insert(&key, &value)?;
        }
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
/// or does not land on a leaf.
fn column_scalar<T: ReadableTable<TableKey, TableValue>>(t: &T, entity: Skey, column: &SPath) -> SdbResult<Scalar> {
    let scalar = match tree::resolve_from(t, entity, column)? {
        Some(key) => tree::leaf_scalar_opt(t, key)?.unwrap_or(Scalar::Null),
        None => Scalar::Null,
    };

    Ok(scalar)
}
