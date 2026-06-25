//! Opaque keys used internally to identify nodes and indexes.

use uuid::Uuid;
use std::fmt::{Debug, Display, Formatter, Result as FmtResult};

/// Opaque, unique primary key identifying a stored node.
///
/// Backed by a time-ordered UUID (v7): unique, fixed 16-byte size, and roughly
/// sortable by creation time. The internal representation is not part of the
/// public API.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Skey(Uuid);

impl Skey {
    /// The fixed primary key of a table's root node; path resolution walks from
    /// here. The nil UUID is distinct from any generated v7 key.
    pub(crate) const ROOT: Skey = Skey(Uuid::nil());

    /// Generates a fresh, time-ordered key.
    pub(crate) fn generate() -> Self {
        Self(Uuid::now_v7())
    }

    /// Returns the raw 16-byte representation (big-endian, order-preserving).
    pub(crate) fn to_bytes(self) -> [u8; 16] {
        *self.0.as_bytes()
    }

    /// Rebuilds a key from its raw 16-byte representation.
    pub(crate) fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(Uuid::from_bytes(bytes))
    }
}

impl Debug for Skey {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "Skey({})", self.0)
    }
}

impl Display for Skey {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        Display::fmt(&self.0, f)
    }
}
