//! StratoDB — a typed, transactional, indexed document store.
//!
//! StratoDB layers a document/tree data model over an embedded key-value
//! engine. Data is fully *shredded* into a tree of nodes — objects, lists and
//! scalar leaves — each bearing its own opaque primary key ([`Skey`]) and
//! addressable by a [`SPath`](path::SPath), a slash-separated path such as `users/alice/age`
//! or `items[3]/name`. Paths are never stored; they resolve by walking the tree
//! at query time, so an entity keeps its identity through renames and moves.
//!
//! The underlying storage engine is an internal implementation detail and is
//! never exposed through the public API.
//!
//! # Getting started
//!
//! A database is one file holding any number of named [`Table`]s; transactions
//! come off a table handle. Reads are concurrent, writes are serialized, and
//! changes become durable only on [`commit`](txn::WriteTxn::commit).
//!
//! ```
//! use stratodb::{NodeKind, StratoDb};
//!
//! # fn main() -> stratodb::SdbResult<()> {
//! // `create_in_memory` keeps everything in RAM; for a file use
//! // `StratoDb::create(path)` (or `StratoDb::open(path)` to reopen one).
//! let db = StratoDb::create_in_memory()?;
//! let config = db.open_table("config")?;
//!
//! let w = config.write()?;
//! w.put("server/host", &String::from("localhost"))?;
//! w.put("server/port", &8080u32)?;
//! w.commit()?;
//!
//! let r = config.read()?;
//! assert_eq!(r.get::<u32>("server/port")?, Some(8080));
//! // Paths address a tree of nodes; `server` itself is an object node.
//! assert_eq!(r.kind("server")?, Some(NodeKind::Object));
//! # Ok(())
//! # }
//! ```
//!
//! # Secondary indexes
//!
//! A named index over a path pattern (`users/*`) keeps an ordered, optionally
//! unique key on one or more columns; a query recomposes each matched entity
//! from its own subtree.
//!
//! ```
//! use std::collections::BTreeMap;
//! use stratodb::{
//!     data::Scalar,
//!     index::{IndexColumn, IndexDef},
//!     path::SPath,
//!     StratoDb,
//! };
//!
//! # fn main() -> stratodb::SdbResult<()> {
//! let db = StratoDb::create_in_memory()?;
//! let users = db.open_table("users")?;
//!
//! // Index the `age` field of every `users/*` entity, ascending.
//! users.create_index(&IndexDef::new(
//!     String::from("by_age"),
//!     String::from("users/*"),
//!     vec![IndexColumn::asc(SPath::parse("age")?)],
//!     false,
//! ))?;
//!
//! let w = users.write()?;
//! w.put("users/alice/age", &30u32)?;
//! w.put("users/bob/age", &30u32)?;
//! w.put("users/carol/age", &40u32)?;
//! w.commit()?;
//!
//! // Both 30-year-olds come back, each recomposed from its subtree.
//! let r = users.read()?;
//! let at_30: Vec<BTreeMap<String, u32>> = r.find("by_age", &[Scalar::U32(30)])?;
//! assert_eq!(at_30.len(), 2);
//! # Ok(())
//! # }
//! ```
//!
//! With the `derive` feature, `#[derive(SData)]` lets a Rust struct store and
//! load directly, and `#[strato(index(...))]` declares its indexes (registered
//! in one call with [`Table::create_indexes`]); see the `indexed` example.
//!
//! # What else is here
//!
//! - **Typed data** — implement [`SData`] by hand or derive it (feature `derive`); see the [`data`] module and the
//!   `basic` / `indexed` examples.
//! - **Dynamic documents** — [`Value`] is a faithful, in-memory mirror of the node tree with path-addressed
//!   `get`/`set`, loaded and stored on a transaction with [`load_value`](txn::ReadTxn::load_value) /
//!   [`store_value`](txn::WriteTxn::store_value).
//! - **Export** — render any stored or in-memory subtree to JSON or YAML through the [`export`] module's `JsonExporter`
//!   / `YamlExporter` traits.
//! - **Big numbers** — `BigInt`, `BigFloat` and `BigRational` as scalars and/or stored data behind the `bignum` feature
//!   family.

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
pub mod export;
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
