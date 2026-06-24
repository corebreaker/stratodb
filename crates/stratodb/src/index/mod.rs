//! Secondary indexes.
//!
//! Built incrementally across milestone 3: order-preserving key encoding, index
//! definitions and the `$metadata` registry are in place; write-time maintenance
//! and queries follow.

mod definitions;
mod id;
mod ordered;
mod pattern;

pub(crate) mod maintenance;
pub(crate) mod registry;

pub(crate) use self::id::IndexId;

pub use self::definitions::{Direction, IndexColumn, IndexDef};
