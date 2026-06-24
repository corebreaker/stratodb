//! The [`SValue`] trait, mapping Rust types to and from a [`Scalar`].

use super::{
    leaf::{Leaf, LeafMut},
    definition::SData,
    Scalar,
};

use crate::{
    access::{Reader, Writer},
    error::{SdbError, SdbResult},
    path::SPath,
};

use chrono::{DateTime, NaiveDate, NaiveTime, TimeDelta, Utc};
use uuid::Uuid;

/// A Rust type that maps to and from a single persisted [`Scalar`] (leaf) value.
pub trait SValue: Sized {
    /// Converts this value into its scalar representation.
    fn to_scalar(&self) -> Scalar;

    /// Reconstructs this value from a stored scalar.
    fn from_scalar(scalar: &Scalar) -> SdbResult<Self>;
}

impl SValue for Scalar {
    fn to_scalar(&self) -> Scalar {
        self.clone()
    }

    fn from_scalar(scalar: &Scalar) -> SdbResult<Self> {
        Ok(scalar.clone())
    }
}

macro_rules! scalar_value {
    ($t:ty, $variant:ident, $name:literal) => {
        impl SValue for $t {
            fn to_scalar(&self) -> Scalar {
                Scalar::$variant(self.clone())
            }

            fn from_scalar(scalar: &Scalar) -> SdbResult<Self> {
                match scalar {
                    Scalar::$variant(v) => Ok(v.clone()),
                    other => Err(SdbError::TypeMismatch {
                        expected: $name,
                        found:    other.type_str(),
                    }),
                }
            }
        }
    };
}

scalar_value!(bool, Bool, "bool");
scalar_value!(i8, I8, "i8");
scalar_value!(i16, I16, "i16");
scalar_value!(i32, I32, "i32");
scalar_value!(i64, I64, "i64");
scalar_value!(i128, I128, "i128");
scalar_value!(u8, U8, "u8");
scalar_value!(u16, U16, "u16");
scalar_value!(u32, U32, "u32");
scalar_value!(u64, U64, "u64");
scalar_value!(u128, U128, "u128");
scalar_value!(f32, F32, "f32");
scalar_value!(f64, F64, "f64");
scalar_value!(String, Str, "str");
scalar_value!(Vec<u8>, Bytes, "bytes");
scalar_value!(Uuid, Uuid, "uuid");
scalar_value!(NaiveDate, Date, "date");
scalar_value!(NaiveTime, Time, "time");
scalar_value!(DateTime<Utc>, DateTime, "datetime");
scalar_value!(TimeDelta, Duration, "duration");

// Platform-dependent integer widths are normalised to a fixed width so the
// on-disk format is portable.
impl SValue for usize {
    fn to_scalar(&self) -> Scalar {
        Scalar::U64(*self as u64)
    }

    fn from_scalar(scalar: &Scalar) -> SdbResult<Self> {
        match scalar {
            Scalar::U64(v) => Ok(*v as usize),
            other => Err(SdbError::TypeMismatch {
                expected: "usize",
                found:    other.type_str(),
            }),
        }
    }
}

impl SValue for isize {
    fn to_scalar(&self) -> Scalar {
        Scalar::I64(*self as i64)
    }

    fn from_scalar(scalar: &Scalar) -> SdbResult<Self> {
        match scalar {
            Scalar::I64(v) => Ok(*v as isize),
            other => Err(SdbError::TypeMismatch {
                expected: "isize",
                found:    other.type_str(),
            }),
        }
    }
}

impl<T: SValue> SValue for Option<T> {
    fn to_scalar(&self) -> Scalar {
        match self {
            Some(v) => v.to_scalar(),
            None => Scalar::Null,
        }
    }

    fn from_scalar(scalar: &Scalar) -> SdbResult<Self> {
        match scalar {
            Scalar::Null => Ok(None),
            other => Ok(Some(T::from_scalar(other)?)),
        }
    }
}

// Every scalar is also `SData`: it stores as a single leaf, so a struct field of
// scalar type and a composite field decompose through the same trait. The impls
// are concrete (not a blanket over `SValue`) to stay coherent with the derived
// and container impls.
macro_rules! scalar_sdata {
    ($t:ty) => {
        impl SData for $t {
            type Mut<'t> = LeafMut<'t, $t>;
            type Ref<'t> = Leaf<'t, $t>;

            fn store<W: Writer>(&self, writer: &W, at: &SPath) -> SdbResult<()> {
                writer.put_scalar(at, self.to_scalar())
            }

            fn load<R: Reader>(reader: &R, at: &SPath) -> SdbResult<Self> {
                match reader.scalar_at(at)? {
                    Some(scalar) => <$t>::from_scalar(&scalar),
                    None => Err(SdbError::PathNotFound(at.clone())),
                }
            }
        }
    };
}

scalar_sdata!(bool);
scalar_sdata!(i8);
scalar_sdata!(i16);
scalar_sdata!(i32);
scalar_sdata!(i64);
scalar_sdata!(i128);
scalar_sdata!(u8);
scalar_sdata!(u16);
scalar_sdata!(u32);
scalar_sdata!(u64);
scalar_sdata!(u128);
scalar_sdata!(f32);
scalar_sdata!(f64);
scalar_sdata!(usize);
scalar_sdata!(isize);
scalar_sdata!(String);
scalar_sdata!(Uuid);
scalar_sdata!(NaiveDate);
scalar_sdata!(NaiveTime);
scalar_sdata!(DateTime<Utc>);
scalar_sdata!(TimeDelta);
