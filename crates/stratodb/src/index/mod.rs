//! Secondary indexes.
//!
//! Order-preserving key encoding, index definitions and the `$metadata` registry,
//! write-time maintenance, exact/prefix queries with direction, and unique
//! enforcement. Types declare their indexes via [`SIndexed`] (from
//! `#[sdata(index(...))]`).

mod definitions;
mod id;
mod indexed;
mod ordered;
mod pattern;

pub(crate) mod maintenance;
pub(crate) mod registry;

pub(crate) use self::{id::IndexId, pattern::Pattern};

pub use self::{
    definitions::{Direction, IndexColumn, IndexDef},
    indexed::SIndexed,
};
