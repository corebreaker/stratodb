//! The database handle and the database-wide shared state.

mod database;
mod inner;

pub(crate) use inner::DbInner;

pub use database::StratoDb;
