use super::{
    super::IndexDef,
    repository::{RegistryRepository, MetaTable},
    IndexEntry,
};

use crate::error::SdbResult;
use redb::ReadableTable;

/// Registers `def` for `table`, returning whether a new index was created.
/// Idempotent (`Ok(false)`) when an identical definition is already registered
/// under the same name; errors with [`SdbError::SchemaMismatch`] if the name is
/// taken by a different definition.
pub(crate) fn create(meta: &mut MetaTable<'_>, table: &str, def: &IndexDef) -> SdbResult<bool> {
    RegistryRepository::create(meta, table, def)
}

/// Looks up an index by `table` and `name`.
pub(crate) fn lookup<T: ReadableTable<&'static str, &'static [u8]>>(
    meta: &T,
    table: &str,
    name: &str,
) -> SdbResult<Option<IndexEntry>> {
    RegistryRepository::lookup(meta, table, name)
}

/// Returns every index registered on `table`.
pub(crate) fn for_table<T: ReadableTable<&'static str, &'static [u8]>>(
    meta: &T,
    table: &str,
) -> SdbResult<Vec<IndexEntry>> {
    RegistryRepository::for_table(meta, table)
}
