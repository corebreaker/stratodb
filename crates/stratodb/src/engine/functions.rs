//! Engine operations: data-table definitions and `$metadata` bootstrap.

use super::{META_TABLE, TableKey, TableValue};
use crate::{
    constants::{FORMAT_VERSION, META_FORMAT_VERSION_KEY},
    error::{SdbError, SdbResult},
};

use redb::{Database, ReadableTable, TableDefinition};

/// Returns the engine table definition for a StratoDB data table.
pub(crate) fn data_def(name: &str) -> TableDefinition<'_, TableKey, TableValue> {
    TableDefinition::new(name)
}

/// Ensures the `$metadata` table exists and that the on-disk format version is
/// compatible with this build. Idempotent.
pub(crate) fn bootstrap_metadata(db: &Database) -> SdbResult<()> {
    let txn = db.begin_write()?;
    {
        let mut table = txn.open_table(META_TABLE)?;
        let current = table.get(META_FORMAT_VERSION_KEY)?.map(|guard| guard.value().to_vec());

        match current {
            None => {
                let version = FORMAT_VERSION.to_be_bytes();
                table.insert(META_FORMAT_VERSION_KEY, version.as_slice())?;
            }
            Some(bytes) => {
                let stored = bytes
                    .as_slice()
                    .try_into()
                    .map(u32::from_be_bytes)
                    .map_err(|_| SdbError::Corrupt("malformed format version".into()))?;

                if stored != FORMAT_VERSION {
                    return Err(SdbError::SchemaMismatch(format!(
                        "on-disk format version {stored}, this build supports {FORMAT_VERSION}"
                    )));
                }
            }
        }
    }

    txn.commit()?;
    Ok(())
}
