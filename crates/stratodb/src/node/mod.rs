//! The node model: the unit addressed by a primary key.

mod definition;
mod kind;

pub(crate) use definition::{tag, Node};

pub use kind::NodeKind;
