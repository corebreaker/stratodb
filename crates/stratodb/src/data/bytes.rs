//! [`Bytes`]: an opaque byte string stored as a single leaf.
//!
//! This is the "packed" escape hatch: unlike `Vec<u8>` (which shreds into one
//! node per byte), `Bytes` persists as a single `Bytes` scalar leaf.

use super::{
    leaf::{Leaf, LeafMut},
    SData,
    SValue,
    Scalar,
};

use crate::{
    access::{Reader, Writer},
    error::{SdbError, SdbResult},
    path::SPath,
};

/// An opaque byte string persisted as a single leaf.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bytes(pub Vec<u8>);

impl SValue for Bytes {
    fn to_scalar(&self) -> Scalar {
        Scalar::Bytes(self.0.clone())
    }

    fn from_scalar(scalar: &Scalar) -> SdbResult<Self> {
        match scalar {
            Scalar::Bytes(bytes) => Ok(Bytes(bytes.clone())),
            other => Err(SdbError::TypeMismatch {
                expected: "bytes",
                found:    other.type_str(),
            }),
        }
    }
}

impl SData for Bytes {
    type Mut<'t> = LeafMut<'t, Bytes>;
    type Ref<'t> = Leaf<'t, Bytes>;

    fn store<W: Writer>(&self, writer: &W, at: &SPath) -> SdbResult<()> {
        writer.put_scalar(at, self.to_scalar())
    }

    fn load<R: Reader>(reader: &R, at: &SPath) -> SdbResult<Self> {
        match reader.scalar_at(at)? {
            Some(scalar) => Bytes::from_scalar(&scalar),
            None => Err(SdbError::PathNotFound(at.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_and_from_scalar() {
        let bytes = Bytes(vec![1, 2, 3]);

        assert_eq!(bytes.to_scalar(), Scalar::Bytes(vec![1, 2, 3]));
        assert_eq!(Bytes::from_scalar(&Scalar::Bytes(vec![1, 2, 3])).unwrap(), bytes);
    }

    #[test]
    fn from_scalar_rejects_a_non_bytes_scalar() {
        assert!(matches!(
            Bytes::from_scalar(&Scalar::I32(1)),
            Err(SdbError::TypeMismatch {
                expected: "bytes",
                ..
            })
        ));
    }
}
