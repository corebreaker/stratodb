//! Secondary indexes.
//!
//! Built incrementally across milestone 3: order-preserving key encoding, index
//! definitions and the `$metadata` registry are in place; write-time maintenance
//! and queries follow.

mod definitions;
mod id;

pub(crate) mod registry;

// The codec is fully exercised by its own tests; its first non-test caller
// arrives with index maintenance (a later milestone-3 sub-step). Remove this
// allow once it is wired in.
#[allow(dead_code)]
pub(crate) mod ordered;

pub(crate) use self::id::IndexId;

pub use self::definitions::{Direction, IndexColumn, IndexDef};
