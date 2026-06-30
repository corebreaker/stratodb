//! The storage-engine boundary.
//!
//! This is the only layer that knows about the concrete key-value engine. It
//! defines the composite key/value types stored in a StratoDB data table and
//! their engine (de)serialization, maps engine errors into [`SdbError`], and owns
//! the reserved `$metadata` table.
//!
//! A single engine table per StratoDB table holds both data nodes and index
//! entries. The leading discriminant of [`TableKey`] partitions the key space
//! into contiguous `Data` and index ranges.

mod archived;
mod backend;
mod errors;
mod functions;
mod table_defs;
mod table_key;
mod table_value;

pub(crate) use self::{
    archived::ArchivedNodes,
    backend::{MemNodes, ReadNodes, WriteNodes},
    table_defs::META_TABLE,
    table_key::TableKey,
    table_value::TableValue,
    functions::{bootstrap_metadata, data_def},
};
