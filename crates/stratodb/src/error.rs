//! Error and result types for the public API.

use crate::path::SPath;
use std::error::Error;

/// Result type used throughout the public API.
pub type SdbResult<T> = Result<T, SdbError>;

/// Errors returned by StratoDB.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SdbError {
    /// An error originating from the underlying storage engine.
    ///
    /// The concrete engine type is intentionally kept private so that the
    /// storage backend remains an implementation detail.
    #[error("storage engine error: {0}")]
    Engine(#[source] Box<dyn Error + Send + Sync + 'static>),

    /// A path string could not be parsed into an [`SPath`].
    #[error("invalid path: {0}")]
    InvalidPath(String),

    /// A table name was rejected (empty or reserved).
    #[error("invalid table name: {0}")]
    InvalidTableName(String),

    /// No node exists at the requested path.
    #[error("path not found: {0}")]
    PathNotFound(SPath),

    /// The node at a path was not of the kind the operation required.
    #[error("unexpected node at '{path}': expected {expected}, found {found}")]
    UnexpectedNode {
        /// The path that was being accessed.
        path:     SPath,
        /// The node kind the operation required.
        expected: &'static str,
        /// The node kind actually stored.
        found:    &'static str,
    },

    /// A scalar could not be read as the requested Rust type.
    #[error("type mismatch: expected {expected}, found {found}")]
    TypeMismatch {
        /// The Rust type the caller asked for.
        expected: &'static str,
        /// The scalar variant actually stored.
        found:    &'static str,
    },

    /// A list index referenced a position past the end of the list.
    #[error("list index out of range at '{path}': {index} (length {len})")]
    IndexOutOfRange {
        /// The list-element path that was rejected.
        path:  SPath,
        /// The requested index.
        index: u64,
        /// The current list length.
        len:   u64,
    },

    /// Stored bytes could not be decoded (corruption or format skew).
    #[error("corrupted data: {0}")]
    Corrupt(String),

    /// A unique index constraint was violated.
    #[error("unique index '{index}' violated")]
    UniqueViolation {
        /// The name of the violated index.
        index: String,
    },

    /// The on-disk format/schema does not match this build.
    #[error("schema mismatch: {0}")]
    SchemaMismatch(String),

    /// A data access cannot be fulfilled.
    #[error("impossible access: {0}")]
    CannotAccess(String),
}

impl SdbError {
    /// Wraps a storage-engine error, keeping its source chain but hiding the
    /// concrete type from the public API.
    pub(crate) fn engine<E>(error: E) -> Self
    where
        E: Error + Send + Sync + 'static, {
        SdbError::Engine(Box::new(error))
    }
}
