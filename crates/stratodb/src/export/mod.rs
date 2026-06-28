//! Read-only rendering of a [`Value`](crate::Value) into textual formats.
//!
//! The [`JsonExporter`] / [`YamlExporter`] traits (in `exporter`) are the public
//! surface; a [`ReadTxn`](crate::txn::ReadTxn) or a [`Value`](crate::Value) loads
//! the addressed subtree and hands it to the JSON or YAML writer. Leaf scalars
//! are projected to text in `scalar` (the only lossy step); everything but the
//! traits is internal.

mod base64;
mod exporter;
mod json;
mod scalar;
mod string;
mod yaml;

pub use self::exporter::{JsonExporter, YamlExporter};
