//! Opaque keys used internally to identify nodes and indexes.

use crate::error::SdbError;
use uuid::Uuid;
use std::{
    cell::Cell,
    fmt::{Debug, Display, Formatter, Result as FmtResult},
    str::FromStr,
};

/// Opaque, unique primary key identifying a stored node.
///
/// A fresh key is 128 random bits drawn from a fast per-thread generator (see
/// [`generate`](Self::generate)): unique, fixed 16-byte size, and cheap to mint.
/// Keys are only ever compared for equality and point-looked-up (never range
/// scanned by value), so no time-ordering is needed — that lets the generator
/// avoid a per-call clock read and OS random draw. The internal representation is
/// not part of the public API.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Skey(Uuid);

thread_local! {
    /// splitmix64 state, seeded once per thread from OS entropy. A dedicated fast
    /// PRNG (not a clock + `getrandom` per key like a UUID v7/v4) — the hot write
    /// path mints several keys per stored entity, so this shaves a real cost.
    static KEY_RNG: Cell<u64> = Cell::new(seed());
}

/// A one-time strong per-thread seed. Drawing a single UUID v7 pulls OS entropy
/// (and the current time) once; every subsequent key comes from the cheap PRNG.
fn seed() -> u64 {
    let bits = Uuid::now_v7().as_u128();
    let mixed = (bits as u64) ^ ((bits >> 64) as u64);

    // Guard against an all-zero state (splitmix64 tolerates it, but stay safe).
    mixed | 1
}

/// One 64-bit splitmix64 draw, advancing `state`.
fn splitmix64(state: &Cell<u64>) -> u64 {
    let next = state.get().wrapping_add(0x9E37_79B9_7F4A_7C15);
    state.set(next);

    let mut z = next;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);

    z ^ (z >> 31)
}

impl Skey {
    /// The fixed primary key of a table's root node; path resolution walks from
    /// here. The nil UUID is distinct from any generated key (which is random).
    pub const ROOT: Skey = Skey(Uuid::nil());

    /// Generates a fresh, random key from the per-thread fast PRNG.
    pub fn generate() -> Self {
        KEY_RNG.with(|state| {
            let hi = splitmix64(state);
            let lo = splitmix64(state);

            Self(Uuid::from_u128((u128::from(hi) << 64) | u128::from(lo)))
        })
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
