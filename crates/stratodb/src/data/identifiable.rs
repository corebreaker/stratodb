//! [`SIdentifiable`]: the identity every accessor carries.

use crate::{path::SPath, Skey};

/// Identity shared by every accessor over a stored node.
///
/// An accessor resolves its node's primary key eagerly when it is built, so
/// [`key`](SIdentifiable::key) is infallible. The key is stable: a node keeps it
/// when the tree around it changes (e.g. a list element keeps its key when
/// preceding siblings are removed), whereas [`path`](SIdentifiable::path)
/// reflects where the node currently lives.
pub trait SIdentifiable: Sized {
    /// The primary key of the node this accessor points at.
    fn key(&self) -> Skey;

    /// The path this accessor was opened at.
    fn path(&self) -> &SPath;
}
