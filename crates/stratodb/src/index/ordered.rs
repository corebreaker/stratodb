//! Order-preserving encoding of scalar values for index keys.
//!
//! A `Scalar` is encoded so that the bytewise (`memcmp`) order of two encodings
//! matches the values' semantic order — the property index keys rely on, since
//! the engine compares index keys bytewise. Each encoding is a 1-byte type tag
//! followed by an order-preserving, self-delimiting body, so per-column
//! encodings concatenate into a composite key and still compare correctly. A
//! descending column is the bitwise complement of its ascending encoding.
//!
//! This is **encode-only**: index queries build key prefixes to scan; they never
//! decode column values back (a lookup returns the stored entity key).

use crate::data::Scalar;

use chrono::{Datelike, Timelike};

/// Type tags. Their relative order fixes the (arbitrary but deterministic)
/// ordering between distinct scalar types; within a type the body decides.
/// `Null` is the smallest so a missing/`Null` column sorts first (ascending).
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

/// Appends `scalar`'s order-preserving encoding to `out`. When `descending`, the
/// freshly written bytes are bit-inverted so the column sorts in reverse.
pub(crate) fn encode_scalar(out: &mut Vec<u8>, scalar: &Scalar, descending: bool) {
    let start = out.len();
    encode_ascending(out, scalar);

    if descending {
        for byte in &mut out[start..] {
            *byte = !*byte;
        }
    }
}

fn encode_ascending(out: &mut Vec<u8>, scalar: &Scalar) {
    match scalar {
        Scalar::Null => out.push(tag::NULL),
        Scalar::Bool(v) => {
            out.push(tag::BOOL);
            out.push(u8::from(*v));
        }
        Scalar::I8(v) => signed(out, tag::I8, &v.to_be_bytes()),
        Scalar::I16(v) => signed(out, tag::I16, &v.to_be_bytes()),
        Scalar::I32(v) => signed(out, tag::I32, &v.to_be_bytes()),
        Scalar::I64(v) => signed(out, tag::I64, &v.to_be_bytes()),
        Scalar::I128(v) => signed(out, tag::I128, &v.to_be_bytes()),
        Scalar::U8(v) => {
            out.push(tag::U8);
            out.push(*v);
        }
        Scalar::U16(v) => unsigned(out, tag::U16, &v.to_be_bytes()),
        Scalar::U32(v) => unsigned(out, tag::U32, &v.to_be_bytes()),
        Scalar::U64(v) => unsigned(out, tag::U64, &v.to_be_bytes()),
        Scalar::U128(v) => unsigned(out, tag::U128, &v.to_be_bytes()),
        Scalar::F32(v) => unsigned(out, tag::F32, &float32(*v).to_be_bytes()),
        Scalar::F64(v) => unsigned(out, tag::F64, &float64(*v).to_be_bytes()),
        Scalar::Str(v) => {
            out.push(tag::STR);
            ordered_bytes(out, v.as_bytes());
        }
        Scalar::Bytes(v) => {
            out.push(tag::BYTES);
            ordered_bytes(out, v);
        }
        Scalar::Uuid(v) => {
            out.push(tag::UUID);
            out.extend_from_slice(v.as_bytes());
        }
        Scalar::Date(v) => signed(out, tag::DATE, &v.num_days_from_ce().to_be_bytes()),
        Scalar::Time(v) => {
            out.push(tag::TIME);
            out.extend_from_slice(&v.num_seconds_from_midnight().to_be_bytes());
            out.extend_from_slice(&v.nanosecond().to_be_bytes());
        }
        Scalar::DateTime(v) => {
            out.push(tag::DATETIME);
            let naive = v.naive_utc();
            push_signed(out, &naive.num_days_from_ce().to_be_bytes());
            out.extend_from_slice(&naive.num_seconds_from_midnight().to_be_bytes());
            out.extend_from_slice(&naive.nanosecond().to_be_bytes());
        }
        Scalar::Duration(v) => {
            // Total nanoseconds as a signed 128-bit value captures the full order
            // (whole TimeDelta range × 1e9 fits in i128).
            let nanos = i128::from(v.num_seconds()) * 1_000_000_000 + i128::from(v.subsec_nanos());
            signed(out, tag::DURATION, &nanos.to_be_bytes());
        }
    }
}

/// Writes `tag` then a sign-flipped big-endian integer (two's-complement order →
/// unsigned byte order: flipping the sign bit maps the minimum to all-zero).
fn signed(out: &mut Vec<u8>, tag: u8, be: &[u8]) {
    out.push(tag);
    push_signed(out, be);
}

fn push_signed(out: &mut Vec<u8>, be: &[u8]) {
    out.push(be[0] ^ 0x80);
    out.extend_from_slice(&be[1..]);
}

/// Writes `tag` then big-endian bytes verbatim (already order-preserving for
/// unsigned integers and the float total-order transform).
fn unsigned(out: &mut Vec<u8>, tag: u8, be: &[u8]) {
    out.push(tag);
    out.extend_from_slice(be);
}

/// IEEE-754 total order: negatives (sign bit set) flip every bit; non-negatives
/// flip only the sign bit. The result compares as unsigned in value order.
fn float64(v: f64) -> u64 {
    let bits = v.to_bits();
    if bits & (1 << 63) != 0 { !bits } else { bits | (1 << 63) }
}

fn float32(v: f32) -> u32 {
    let bits = v.to_bits();
    if bits & (1 << 31) != 0 { !bits } else { bits | (1 << 31) }
}

/// Order-preserving, self-delimiting byte string: each `0x00` is escaped as
/// `0x00 0xFF`, and the whole is terminated by `0x00 0x01`.
///
/// The two-byte terminator (rather than a bare `0x00`) is what makes encodings
/// **prefix-free**: after any `0x00` the next byte is `0xFF` (escaped content) or
/// `0x01` (terminator), and `0x01 < 0xFF`, so no encoding is a prefix of
/// another. That matters for descending columns — `encode_scalar` reverses a
/// column by bit-complementing it, and bit-complement only reverses order when
/// neither operand is a prefix of the other (a bare `0x00` terminator would make
/// the empty string a prefix of `"\0"`, breaking descending order).
fn ordered_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    for &byte in bytes {
        out.push(byte);
        if byte == 0x00 {
            out.push(0xFF);
        }
    }

    out.push(0x00);
    out.push(0x01);
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::{DateTime, NaiveDate, NaiveTime, TimeDelta, Utc};
    use uuid::Uuid;

    fn asc(scalar: &Scalar) -> Vec<u8> {
        let mut out = Vec::new();
        encode_scalar(&mut out, scalar, false);
        out
    }

    fn desc(scalar: &Scalar) -> Vec<u8> {
        let mut out = Vec::new();
        encode_scalar(&mut out, scalar, true);
        out
    }

    /// Asserts the scalars are given in strictly ascending value order, and that
    /// the encoding preserves it (ascending) and reverses it (descending).
    fn assert_ordered(sorted: &[Scalar]) {
        for pair in sorted.windows(2) {
            let (lo, hi) = (&pair[0], &pair[1]);
            assert!(asc(lo) < asc(hi), "ascending: {lo:?} should encode below {hi:?}");
            assert!(desc(lo) > desc(hi), "descending: {lo:?} should encode above {hi:?}");
        }
    }

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn datetime(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> DateTime<Utc> {
        date(y, m, d).and_hms_opt(h, min, s).unwrap().and_utc()
    }

    #[test]
    fn signed_integers_order() {
        assert_ordered(&[
            Scalar::I32(i32::MIN),
            Scalar::I32(-2),
            Scalar::I32(-1),
            Scalar::I32(0),
            Scalar::I32(1),
            Scalar::I32(i32::MAX),
        ]);
        assert_ordered(&[Scalar::I128(i128::MIN), Scalar::I128(0), Scalar::I128(i128::MAX)]);
        assert_ordered(&[Scalar::I8(i8::MIN), Scalar::I8(0), Scalar::I8(i8::MAX)]);
    }

    #[test]
    fn unsigned_integers_order() {
        assert_ordered(&[Scalar::U64(0), Scalar::U64(1), Scalar::U64(u64::MAX)]);
        assert_ordered(&[Scalar::U8(0), Scalar::U8(128), Scalar::U8(u8::MAX)]);
    }

    #[test]
    fn booleans_order() {
        assert_ordered(&[Scalar::Bool(false), Scalar::Bool(true)]);
    }

    #[test]
    fn floats_order() {
        assert_ordered(&[
            Scalar::F64(f64::NEG_INFINITY),
            Scalar::F64(-100.5),
            Scalar::F64(-1.0),
            Scalar::F64(0.0),
            Scalar::F64(1.0),
            Scalar::F64(100.5),
            Scalar::F64(f64::INFINITY),
        ]);
        assert_ordered(&[Scalar::F32(-1.0), Scalar::F32(0.0), Scalar::F32(1.0)]);
    }

    #[test]
    fn strings_order() {
        assert_ordered(&[
            Scalar::Str(String::new()),
            Scalar::Str("a".into()),
            Scalar::Str("ab".into()),
            Scalar::Str("b".into()),
        ]);
    }

    #[test]
    fn strings_with_interior_null_byte_stay_ordered() {
        // The escape (0x00 -> 0x00 0xFF) + 0x00 terminator must keep prefixes
        // below extensions and "\0" below ordinary bytes.
        assert_ordered(&[
            Scalar::Str("a".into()),
            Scalar::Str("a\u{0}".into()),
            Scalar::Str("a\u{0}b".into()),
            Scalar::Str("ab".into()),
        ]);
    }

    #[test]
    fn bytes_order() {
        assert_ordered(&[
            Scalar::Bytes(vec![]),
            Scalar::Bytes(vec![0]),
            Scalar::Bytes(vec![0, 0]),
            Scalar::Bytes(vec![1]),
            Scalar::Bytes(vec![255]),
        ]);
    }

    #[test]
    fn uuids_order() {
        assert_ordered(&[
            Scalar::Uuid(Uuid::from_u128(0)),
            Scalar::Uuid(Uuid::from_u128(1)),
            Scalar::Uuid(Uuid::from_u128(u128::MAX)),
        ]);
    }

    #[test]
    fn temporal_order() {
        assert_ordered(&[
            Scalar::Date(date(1, 1, 1)),
            Scalar::Date(date(1969, 12, 31)),
            Scalar::Date(date(2026, 6, 22)),
        ]);
        assert_ordered(&[
            Scalar::Time(NaiveTime::from_hms_opt(0, 0, 0).unwrap()),
            Scalar::Time(NaiveTime::from_hms_milli_opt(12, 0, 0, 1).unwrap()),
            Scalar::Time(NaiveTime::from_hms_opt(23, 59, 59).unwrap()),
        ]);
        assert_ordered(&[
            Scalar::DateTime(datetime(1970, 1, 1, 0, 0, 0)),
            Scalar::DateTime(datetime(1970, 1, 1, 0, 0, 1)),
            Scalar::DateTime(datetime(2026, 6, 22, 12, 0, 0)),
        ]);
        assert_ordered(&[
            Scalar::Duration(TimeDelta::seconds(-10)),
            Scalar::Duration(TimeDelta::seconds(-1) + TimeDelta::nanoseconds(-1)),
            Scalar::Duration(TimeDelta::zero()),
            Scalar::Duration(TimeDelta::nanoseconds(1)),
            Scalar::Duration(TimeDelta::seconds(10)),
        ]);
    }

    #[test]
    fn null_sorts_before_every_other_type() {
        let null = asc(&Scalar::Null);
        for other in [
            Scalar::Bool(false),
            Scalar::I32(i32::MIN),
            Scalar::U8(0),
            Scalar::F64(f64::NEG_INFINITY),
            Scalar::Str(String::new()),
            Scalar::Bytes(vec![]),
        ] {
            assert!(null < asc(&other), "Null should sort before {other:?}");
        }
    }

    #[test]
    fn composite_keys_compare_column_by_column() {
        // Build a 2-column key (string, i32) by concatenating per-column encodings.
        fn composite(name: &str, n: i32) -> Vec<u8> {
            let mut out = Vec::new();
            encode_scalar(&mut out, &Scalar::Str(name.into()), false);
            encode_scalar(&mut out, &Scalar::I32(n), false);
            out
        }

        // The first column dominates; the second breaks ties — even when the
        // second would order the other way on its own.
        assert!(composite("a", 100) < composite("ab", 1));
        assert!(composite("a", 1) < composite("a", 2));
        assert!(composite("a", 2) < composite("b", 1));
    }
}
