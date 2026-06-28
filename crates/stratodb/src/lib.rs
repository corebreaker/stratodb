//! StratoDB — a typed, transactional, indexed document store.
//!
//! StratoDB layers a document/tree data model over an embedded key-value
//! engine. Data is fully shredded into a tree of nodes (objects, lists and
//! scalar leaves), each addressable both by an opaque primary key and by a
//! [`SPath`] (a slash-separated path, e.g. `a/b[12]/x`).
//!
//! The underlying storage engine is an internal implementation detail and is
//! never exposed through the public API.
//!
//! # Example
//!
//! ```no_run
//! use stratodb::StratoDb;
//!
//! # fn main() -> stratodb::SdbResult<()> {
//! let db = StratoDb::create("data.stratodb")?;
//! let users = db.open_table("users")?;
//!
//! let w = users.write()?;
//! w.put("alice/age", &30u32)?;
//! w.commit()?;
//!
//! let r = users.read()?;
//! let age: Option<u32> = r.get("alice/age")?;
//! assert_eq!(age, Some(30));
//! # Ok(())
//! # }
//! ```

mod cache;
mod codec;
mod datetime;
mod db;
mod engine;
mod key;
mod node;
mod table;
mod tree;
mod value;

pub mod access;
pub mod constants;
pub mod data;
pub mod error;
pub mod index;
pub mod path;
pub mod txn;

pub use self::{
    data::SData,
    db::StratoDb,
    error::{SdbError, SdbResult},
    key::Skey,
    node::NodeKind,
    table::Table,
    value::Value,
};

/// Derives [`SData`] for a struct, generating its lazy `StratoXxx` /
/// `StratoXxxMut` accessors. Shares the `SData` name with the trait (distinct
/// namespaces), so `use stratodb::SData;` brings both into scope.
#[cfg(feature = "derive")]
pub use stratodb_derive::SData;
