//! Opaque keys used internally to identify nodes and indexes.

use crate::error::SdbError;
use uuid::Uuid;
use std::{
    fmt::{Debug, Display, Formatter, Result as FmtResult},
    str::FromStr,
};

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
    pub const ROOT: Skey = Skey(Uuid::nil());

    /// Generates a fresh, time-ordered key.
    pub fn generate() -> Self {
        Self(Uuid::now_v7())
    }

    /// Returns the raw 16-byte representation (big-endian, order-preserving).
    pub fn into_bytes(self) -> [u8; 16] {
        *self.0.as_bytes()
    }

    /// Rebuilds a key from its raw 16-byte representation.
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(Uuid::from_bytes(bytes))
    }

    /// Rebuilds a key from a variable-length byte slice, returning
    /// [`SdbError::BadKey`] if the slice is not exactly 16 bytes long.
    pub fn try_from_bytes(bytes: &[u8]) -> Result<Self, SdbError> {
        if bytes.len() != 16 {
            return Err(SdbError::BadKey(String::from("bytes with length different from 16")));
        }

        let mut array = [0u8; 16];
        array.copy_from_slice(bytes);

        Ok(Self::from_bytes(array))
    }
}

impl From<u128> for Skey {
    fn from(v: u128) -> Self {
        Self(Uuid::from_u128(v))
    }
}

impl From<Skey> for u128 {
    fn from(val: Skey) -> Self {
        val.0.as_u128()
    }
}

impl From<Uuid> for Skey {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<Skey> for Uuid {
    fn from(val: Skey) -> Self {
        val.0
    }
}

impl From<[u8; 16]> for Skey {
    fn from(v: [u8; 16]) -> Self {
        Self::from_bytes(v)
    }
}

impl TryFrom<Vec<u8>> for Skey {
    type Error = SdbError;

    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        Self::try_from_bytes(&bytes)
    }
}

impl From<Skey> for Vec<u8> {
    fn from(val: Skey) -> Self {
        val.into_bytes().to_vec()
    }
}

impl FromStr for Skey {
    type Err = SdbError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s)
            .map(Self)
            .map_err(|err| SdbError::BadKey(err.to_string()))
    }
}

impl Debug for Skey {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "Skey({})", self.0)
    }
}

impl Display for Skey {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "key:{}", self.0)
    }
}
