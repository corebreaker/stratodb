//! [`SRef`]: the read-accessor trait.

use super::identifiable::SIdentifiable;
use crate::{access::Reader, path::SPath, Skey};
use std::sync::Arc;

/// A read accessor over a single node.
///
/// Implemented by every generated `StratoXXX` read type and by
/// [`Leaf`](super::leaf::Leaf). An accessor holds a shared [`Reader`] (so the
/// getters of a parent accessor can cheaply hand the same handle to their
/// children), plus the node's path and its already-resolved primary key.
pub trait SRef<'t>: SIdentifiable {
    /// Builds an accessor over the node already resolved to `key` at `base`.
    ///
    /// Called by `ReadTxn::fetch` and by the field getters a parent accessor
    /// exposes; it performs no I/O of its own.
    fn open(reader: Arc<dyn Reader + 't>, base: SPath, key: Skey) -> Self;
}
