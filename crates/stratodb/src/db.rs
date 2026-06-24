//! The database handle.

use redb::Database;
use std::{path::Path, sync::Arc};
use crate::{
    constants::METADATA_TABLE_NAME,
    engine,
    error::{SdbError, SdbResult},
    Table,
};

/// A StratoDB database: a single file holding one or more tables.
///
/// The handle is cheap to clone (it shares the underlying database) and is safe
/// to use concurrently from multiple threads.
#[derive(Clone)]
pub struct StratoDb {
    db: Arc<Database>,
}

impl StratoDb {
    /// Opens the database at `path`, creating it if it does not exist.
    pub fn create(path: impl AsRef<Path>) -> SdbResult<Self> {
        let db = Database::create(path)?;
        engine::bootstrap_metadata(&db)?;

        Ok(Self {
            db: Arc::new(db)
        })
    }

    /// Opens an existing database at `path`.
    pub fn open(path: impl AsRef<Path>) -> SdbResult<Self> {
        let db = Database::open(path)?;
        engine::bootstrap_metadata(&db)?;

        Ok(Self {
            db: Arc::new(db)
        })
    }

    /// Returns a handle to the named table. The table is created on first write.
    pub fn open_table(&self, name: &str) -> SdbResult<Table> {
        if name.is_empty() {
            return Err(SdbError::InvalidTableName("table name must not be empty".into()));
        }

        if name == METADATA_TABLE_NAME {
            return Err(SdbError::InvalidTableName(format!("'{name}' is reserved")));
        }

        Ok(Table::new(self.db.clone(), name.to_string()))
    }
}
