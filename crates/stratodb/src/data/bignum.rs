//! [`SData`] for the big-number types when they are stored as composite data
//! rather than as a native [`Scalar`] variant.
//!
//! These impls are compiled only for a `*-as-data` feature whose matching
//! `*-as-scalar` feature is **off**: in that configuration the type is not a
//! `Scalar`, so it cannot persist as a single native leaf. Instead each value
//! is serialised to bytes and stored as one `Bytes` leaf, and the read/write
//! accessors are those of [`Bytes`] (`get()` yields the raw [`Bytes`]; recompose
//! the typed value with `txn.load::<T>(path)`).

use super::{
    leaf::{Leaf, LeafMut},
    Bytes,
    SData,
    Scalar,
};

use crate::{
    access::{Reader, Writer},
    error::{SdbError, SdbResult},
    path::SPath,
};

#[cfg(all(not(feature = "bigint-as-scalar"), feature = "bigint-as-data"))]
use num_bigint::BigInt;

#[cfg(all(not(feature = "bigfloat-as-scalar"), feature = "bigfloat-as-data"))]
use num_bigfloat::{BigFloat, INF_NEG, INF_POS, NAN, ZERO};

#[cfg(all(not(feature = "rational-as-scalar"), feature = "rational-as-data"))]
use num_rational::BigRational;

/// Reads the single `Bytes` leaf a big-number value was stored as.
fn load_leaf_bytes<R: Reader>(reader: &R, at: &SPath) -> SdbResult<Vec<u8>> {
    match reader.scalar_at(at)? {
        Some(Scalar::Bytes(bytes)) => Ok(bytes),
        Some(other) => Err(SdbError::TypeMismatch {
            expected: "bignum-bytes",
            found:    other.type_str(),
        }),
        None => Err(SdbError::PathNotFound(at.clone())),
    }
}

#[cfg(all(not(feature = "bigint-as-scalar"), feature = "bigint-as-data"))]
impl SData for BigInt {
    type Mut<'t> = LeafMut<'t, Bytes>;
    type Ref<'t> = Leaf<'t, Bytes>;

    fn store<W: Writer>(&self, writer: &W, at: &SPath) -> SdbResult<()> {
        writer.put_scalar(at, Scalar::Bytes(self.to_signed_bytes_be()))
    }

    fn load<R: Reader>(reader: &R, at: &SPath) -> SdbResult<Self> {
        Ok(BigInt::from_signed_bytes_be(&load_leaf_bytes(reader, at)?))
    }
}

#[cfg(all(not(feature = "rational-as-scalar"), feature = "rational-as-data"))]
impl SData for BigRational {
    type Mut<'t> = LeafMut<'t, Bytes>;
    type Ref<'t> = Leaf<'t, Bytes>;

    fn store<W: Writer>(&self, writer: &W, at: &SPath) -> SdbResult<()> {
        let mut bytes = Vec::new();
        crate::codec::put_bytes(&mut bytes, &self.numer().to_signed_bytes_be());
        crate::codec::put_bytes(&mut bytes, &self.denom().to_signed_bytes_be());

        writer.put_scalar(at, Scalar::Bytes(bytes))
    }

    fn load<R: Reader>(reader: &R, at: &SPath) -> SdbResult<Self> {
        let bytes = load_leaf_bytes(reader, at)?;
        let mut reader = crate::codec::Reader::new(&bytes);

        let numer = num_bigint::BigInt::from_signed_bytes_be(reader.bytes()?);
        let denom = num_bigint::BigInt::from_signed_bytes_be(reader.bytes()?);

        Ok(BigRational::new(numer, denom))
    }
}

#[cfg(all(not(feature = "bigfloat-as-scalar"), feature = "bigfloat-as-data"))]
impl SData for BigFloat {
    type Mut<'t> = LeafMut<'t, Bytes>;
    type Ref<'t> = Leaf<'t, Bytes>;

    fn store<W: Writer>(&self, writer: &W, at: &SPath) -> SdbResult<()> {
        writer.put_scalar(at, Scalar::Bytes(bigfloat::to_bytes(self)))
    }

    fn load<R: Reader>(reader: &R, at: &SPath) -> SdbResult<Self> {
        bigfloat::from_bytes(&load_leaf_bytes(reader, at)?)
    }
}

/// Self-contained byte encoding for a [`BigFloat`] stored as data.
///
/// A leading tag byte distinguishes the special values; a finite, non-zero
/// number is `[sign, exponent, mantissa…]` where `exponent` is the raw `i8`
/// reinterpreted as a byte and `mantissa` is the remainder of the buffer (one
/// decimal digit per byte, most significant first).
#[cfg(all(not(feature = "bigfloat-as-scalar"), feature = "bigfloat-as-data"))]
mod bigfloat {
    use super::{BigFloat, INF_NEG, INF_POS, NAN, ZERO};
    use crate::error::{SdbError, SdbResult};

    const NAN_TAG: u8 = 0;
    const INF_POS_TAG: u8 = 1;
    const INF_NEG_TAG: u8 = 2;
    const ZERO_TAG: u8 = 3;
    const NEG_TAG: u8 = 4;
    const POS_TAG: u8 = 5;

    pub(super) fn to_bytes(v: &BigFloat) -> Vec<u8> {
        let mut out = Vec::new();

        // Same ordered checks as the scalar encoding: NaN and the infinities
        // must be caught before the sign/zero tests, which do not describe them.
        if v.is_nan() {
            out.push(NAN_TAG);
        } else if v.is_inf_pos() {
            out.push(INF_POS_TAG);
        } else if v.is_inf_neg() {
            out.push(INF_NEG_TAG);
        } else if v.is_zero() {
            out.push(ZERO_TAG);
        } else {
            out.push(if v.is_negative() { NEG_TAG } else { POS_TAG });
            out.push(v.get_exponent() as u8);

            let mut mantissa = vec![0u8; v.get_mantissa_len()];
            v.get_mantissa_bytes(&mut mantissa);
            out.extend_from_slice(&mantissa);
        }

        out
    }

    pub(super) fn from_bytes(bytes: &[u8]) -> SdbResult<BigFloat> {
        let corrupt = || SdbError::Corrupt("invalid bigfloat encoding".into());
        let (&tag, rest) = bytes.split_first().ok_or_else(corrupt)?;

        let value = match tag {
            NAN_TAG => NAN,
            INF_POS_TAG => INF_POS,
            INF_NEG_TAG => INF_NEG,
            ZERO_TAG => ZERO,
            NEG_TAG | POS_TAG => {
                let (&exponent, mantissa) = rest.split_first().ok_or_else(corrupt)?;
                let sign = if tag == NEG_TAG { -1 } else { 1 };

                BigFloat::from_bytes(mantissa, sign, exponent as i8)
            }
            _ => return Err(corrupt()),
        };

        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StratoDb;

    /// Stores `value` through the `SData` impl under test and reads it back, so
    /// the assertions exercise the real bytes-leaf store/load path end to end.
    fn store_then_load<T: SData>(value: &T) -> T {
        let db = StratoDb::create_in_memory().expect("create db");
        let table = db.open_table("data").expect("open table");

        let w = table.write().expect("write txn");
        w.store("v", value).expect("store");
        w.commit().expect("commit");

        table.read().expect("read txn").load::<T>("v").expect("load")
    }

    #[cfg(all(not(feature = "bigint-as-scalar"), feature = "bigint-as-data"))]
    #[test]
    fn bigint_roundtrips_as_data() {
        let cases = [
            BigInt::from(0),
            BigInt::from(255),
            BigInt::from(-1),
            BigInt::parse_bytes(b"-987654321098765432109876543210", 10).unwrap(),
        ];

        for value in cases {
            assert_eq!(store_then_load(&value), value);
        }
    }

    #[cfg(all(not(feature = "rational-as-scalar"), feature = "rational-as-data"))]
    #[test]
    fn rational_roundtrips_as_data() {
        let value = BigRational::new(num_bigint::BigInt::from(-22), num_bigint::BigInt::from(7));

        assert_eq!(store_then_load(&value), value);
    }

    #[cfg(all(not(feature = "bigfloat-as-scalar"), feature = "bigfloat-as-data"))]
    #[test]
    fn bigfloat_roundtrips_as_data() {
        for value in [
            ZERO,
            INF_POS,
            INF_NEG,
            BigFloat::from_f64(123.5),
            BigFloat::from_f64(-0.001),
        ] {
            assert_eq!(store_then_load(&value), value);
        }

        // NaN never equals itself, so assert the flavour explicitly.
        assert!(store_then_load(&NAN).is_nan());
    }
}
