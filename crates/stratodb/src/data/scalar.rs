//! Scalar leaf values: the [`Scalar`] enum and its storage encoding.

use crate::{
    codec::{self, Reader},
    error::{SdbError, SdbResult},
    datetime::decode_time,
};

use uuid::Uuid;
use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, Timelike, Utc};

mod tag {
    pub(super) const NULL: u8 = 0;
    pub(super) const BOOL: u8 = 1;
    pub(super) const I8: u8 = 2;
    pub(super) const I16: u8 = 3;
    pub(super) const I32: u8 = 4;
    pub(super) const I64: u8 = 5;
    pub(super) const I128: u8 = 6;
    pub(super) const U8: u8 = 7;
    pub(super) const U16: u8 = 8;
    pub(super) const U32: u8 = 9;
    pub(super) const U64: u8 = 10;
    pub(super) const U128: u8 = 11;
    pub(super) const F32: u8 = 12;
    pub(super) const F64: u8 = 13;
    pub(super) const STR: u8 = 14;
    pub(super) const BYTES: u8 = 15;
    pub(super) const UUID: u8 = 16;
    pub(super) const DATE: u8 = 17;
    pub(super) const TIME: u8 = 18;
    pub(super) const DATETIME: u8 = 19;
    pub(super) const DURATION: u8 = 20;
}

/// A persisted scalar value: the content of a leaf node.
///
/// This is the dynamic, runtime representation of any value StratoDB can store
/// in a leaf. Rust types map to and from it through the [`SValue`] trait.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Scalar {
    /// The absence of a value.
    Null,
    /// A boolean.
    Bool(bool),
    /// A signed 8-bit integer.
    I8(i8),
    /// A signed 16-bit integer.
    I16(i16),
    /// A signed 32-bit integer.
    I32(i32),
    /// A signed 64-bit integer.
    I64(i64),
    /// A signed 128-bit integer.
    I128(i128),
    /// An unsigned 8-bit integer.
    U8(u8),
    /// An unsigned 16-bit integer.
    U16(u16),
    /// An unsigned 32-bit integer.
    U32(u32),
    /// An unsigned 64-bit integer.
    U64(u64),
    /// An unsigned 128-bit integer.
    U128(u128),
    /// A 32-bit floating point number.
    F32(f32),
    /// A 64-bit floating point number.
    F64(f64),
    /// A UTF-8 string.
    Str(String),
    /// An opaque byte string.
    Bytes(Vec<u8>),
    /// A free-standing UUID value (not a node key).
    Uuid(Uuid),
    /// A calendar date with no time zone.
    Date(NaiveDate),
    /// A wall-clock time with no time zone.
    Time(NaiveTime),
    /// A date and time with no time zone.
    DateTime(DateTime<Utc>),
    /// A signed duration.
    Duration(TimeDelta),
}

impl Scalar {
    /// A short, stable label for the variant, used in diagnostics.
    pub(crate) fn type_str(&self) -> &'static str {
        match self {
            Scalar::Null => "null",
            Scalar::Bool(_) => "bool",
            Scalar::I8(_) => "i8",
            Scalar::I16(_) => "i16",
            Scalar::I32(_) => "i32",
            Scalar::I64(_) => "i64",
            Scalar::I128(_) => "i128",
            Scalar::U8(_) => "u8",
            Scalar::U16(_) => "u16",
            Scalar::U32(_) => "u32",
            Scalar::U64(_) => "u64",
            Scalar::U128(_) => "u128",
            Scalar::F32(_) => "f32",
            Scalar::F64(_) => "f64",
            Scalar::Str(_) => "str",
            Scalar::Bytes(_) => "bytes",
            Scalar::Uuid(_) => "uuid",
            Scalar::Date(_) => "date",
            Scalar::Time(_) => "time",
            Scalar::DateTime(_) => "datetime",
            Scalar::Duration(_) => "duration",
        }
    }

    /// Appends the exact (round-trippable) storage encoding of this scalar.
    pub(crate) fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            Scalar::Null => buf.push(tag::NULL),
            Scalar::Bool(v) => {
                buf.push(tag::BOOL);
                buf.push(u8::from(*v));
            }
            Scalar::I8(v) => {
                buf.push(tag::I8);
                buf.extend_from_slice(&v.to_be_bytes());
            }
            Scalar::I16(v) => {
                buf.push(tag::I16);
                buf.extend_from_slice(&v.to_be_bytes());
            }
            Scalar::I32(v) => {
                buf.push(tag::I32);
                buf.extend_from_slice(&v.to_be_bytes());
            }
            Scalar::I64(v) => {
                buf.push(tag::I64);
                buf.extend_from_slice(&v.to_be_bytes());
            }
            Scalar::I128(v) => {
                buf.push(tag::I128);
                buf.extend_from_slice(&v.to_be_bytes());
            }
            Scalar::U8(v) => {
                buf.push(tag::U8);
                buf.push(*v);
            }
            Scalar::U16(v) => {
                buf.push(tag::U16);
                buf.extend_from_slice(&v.to_be_bytes());
            }
            Scalar::U32(v) => {
                buf.push(tag::U32);
                buf.extend_from_slice(&v.to_be_bytes());
            }
            Scalar::U64(v) => {
                buf.push(tag::U64);
                buf.extend_from_slice(&v.to_be_bytes());
            }
            Scalar::U128(v) => {
                buf.push(tag::U128);
                buf.extend_from_slice(&v.to_be_bytes());
            }
            Scalar::F32(v) => {
                buf.push(tag::F32);
                buf.extend_from_slice(&v.to_be_bytes());
            }
            Scalar::F64(v) => {
                buf.push(tag::F64);
                buf.extend_from_slice(&v.to_be_bytes());
            }
            Scalar::Str(v) => {
                buf.push(tag::STR);
                codec::put_bytes(buf, v.as_bytes());
            }
            Scalar::Bytes(v) => {
                buf.push(tag::BYTES);
                codec::put_bytes(buf, v);
            }
            Scalar::Uuid(v) => {
                buf.push(tag::UUID);
                buf.extend_from_slice(v.as_bytes());
            }
            Scalar::Date(v) => {
                buf.push(tag::DATE);
                buf.extend_from_slice(&v.num_days_from_ce().to_be_bytes());
            }
            Scalar::Time(v) => {
                buf.push(tag::TIME);
                buf.extend_from_slice(&v.num_seconds_from_midnight().to_be_bytes());
                buf.extend_from_slice(&v.nanosecond().to_be_bytes());
            }
            Scalar::DateTime(v) => {
                buf.push(tag::DATETIME);
                buf.extend_from_slice(&v.naive_utc().num_days_from_ce().to_be_bytes());
                buf.extend_from_slice(&v.time().num_seconds_from_midnight().to_be_bytes());
                buf.extend_from_slice(&v.time().nanosecond().to_be_bytes());
            }
            Scalar::Duration(v) => {
                buf.push(tag::DURATION);
                buf.extend_from_slice(&v.num_seconds().to_be_bytes());
                buf.extend_from_slice(&v.subsec_nanos().to_be_bytes());
            }
        }
    }

    /// Decodes a scalar previously written by [`Scalar::encode`].
    pub(crate) fn decode(r: &mut Reader<'_>) -> SdbResult<Scalar> {
        let tag = r.u8()?;
        let scalar = match tag {
            tag::NULL => Scalar::Null,
            tag::BOOL => Scalar::Bool(r.u8()? != 0),
            tag::I8 => Scalar::I8(i8::from_be_bytes(r.array()?)),
            tag::I16 => Scalar::I16(i16::from_be_bytes(r.array()?)),
            tag::I32 => Scalar::I32(i32::from_be_bytes(r.array()?)),
            tag::I64 => Scalar::I64(i64::from_be_bytes(r.array()?)),
            tag::I128 => Scalar::I128(i128::from_be_bytes(r.array()?)),
            tag::U8 => Scalar::U8(r.u8()?),
            tag::U16 => Scalar::U16(u16::from_be_bytes(r.array()?)),
            tag::U32 => Scalar::U32(r.u32()?),
            tag::U64 => Scalar::U64(r.u64()?),
            tag::U128 => Scalar::U128(u128::from_be_bytes(r.array()?)),
            tag::F32 => Scalar::F32(f32::from_be_bytes(r.array()?)),
            tag::F64 => Scalar::F64(f64::from_be_bytes(r.array()?)),
            tag::STR => {
                let bytes = r.bytes()?;
                let s = std::str::from_utf8(bytes)
                    .map_err(|_| SdbError::Corrupt("invalid utf-8 in string scalar".into()))?;

                Scalar::Str(s.to_string())
            }
            tag::BYTES => Scalar::Bytes(r.bytes()?.to_vec()),
            tag::UUID => Scalar::Uuid(Uuid::from_bytes(r.array()?)),
            tag::DATE => {
                let days = i32::from_be_bytes(r.array()?);
                let date = NaiveDate::from_num_days_from_ce_opt(days)
                    .ok_or_else(|| SdbError::Corrupt("out-of-range date scalar".into()))?;

                Scalar::Date(date)
            }
            tag::TIME => Scalar::Time(decode_time(r)?),
            tag::DATETIME => {
                let days = i32::from_be_bytes(r.array()?);
                let date = NaiveDate::from_num_days_from_ce_opt(days)
                    .ok_or_else(|| SdbError::Corrupt("out-of-range datetime scalar".into()))?;
                let time = decode_time(r)?;

                Scalar::DateTime(NaiveDateTime::new(date, time).and_utc())
            }
            tag::DURATION => {
                let secs = i64::from_be_bytes(r.array()?);
                let nanos = i32::from_be_bytes(r.array()?);

                Scalar::Duration(TimeDelta::seconds(secs) + TimeDelta::nanoseconds(i64::from(nanos)))
            }
            other => return Err(SdbError::Corrupt(format!("unknown scalar tag {other}"))),
        };

        Ok(scalar)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(scalar: Scalar) {
        let mut buf = Vec::new();
        scalar.encode(&mut buf);

        let mut reader = Reader::new(&buf);
        let decoded = Scalar::decode(&mut reader).expect("decode");

        assert!(reader.is_empty(), "trailing bytes for {scalar:?}");
        assert_eq!(scalar, decoded);
    }

    #[test]
    fn scalar_roundtrips() {
        roundtrip(Scalar::Null);
        roundtrip(Scalar::Bool(true));
        roundtrip(Scalar::I32(-42));
        roundtrip(Scalar::I128(i128::MIN));
        roundtrip(Scalar::U64(u64::MAX));
        roundtrip(Scalar::F64(-1.5));
        roundtrip(Scalar::Str(String::from("héllo/[world]")));
        roundtrip(Scalar::Bytes(vec![0, 1, 2, 255]));
        roundtrip(Scalar::Uuid(Uuid::from_u128(0x1234_5678)));
        roundtrip(Scalar::Date(NaiveDate::from_ymd_opt(2026, 6, 21).unwrap()));
        roundtrip(Scalar::Time(NaiveTime::from_hms_milli_opt(23, 59, 59, 250).unwrap()));
        roundtrip(Scalar::DateTime(
            NaiveDate::from_ymd_opt(1970, 1, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc(),
        ));

        roundtrip(Scalar::Duration(
            TimeDelta::seconds(-90) + TimeDelta::milliseconds(-500),
        ));

        roundtrip(Scalar::Duration(TimeDelta::nanoseconds(1)));
    }
}
