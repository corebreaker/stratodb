//! [`SMut`]: the write-accessor trait.

use super::identifiable::SIdentifiable;
use crate::{access::Writer, path::SPath, Skey};
use std::sync::Arc;

/// A write accessor over a single node.
///
/// The mutable counterpart of [`SRef`](super::refs::SRef): implemented by every
/// generated `StratoXXXMut` type and by [`LeafMut`](super::leaf::LeafMut). It
/// holds a shared [`Writer`] plus the node's path and already-resolved primary
/// key, and exposes setters in addition to the read getters.
pub trait SMut<'t>: SIdentifiable {
    /// Builds an accessor over the node already resolved to `key` at `base`.
    ///
    /// Called by `WriteTxn::fetch_mut` and by the field getters a parent
    /// accessor exposes; it performs no I/O of its own.
    fn open(writer: Arc<dyn Writer + 't>, base: SPath, key: Skey) -> Self;
}
