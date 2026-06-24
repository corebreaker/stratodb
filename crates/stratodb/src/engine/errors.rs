//! Conversions from the storage engine's error types into [`SdbError`].

use crate::error::SdbError;

macro_rules! impl_from_redb_error {
    ($($t:ty),* $(,)?) => {
        $(impl From<$t> for SdbError {
            fn from(error: $t) -> Self {
                SdbError::engine(error)
            }
        })*
    };
}

impl_from_redb_error!(
    redb::DatabaseError,
    redb::TransactionError,
    redb::TableError,
    redb::StorageError,
    redb::CommitError,
);
