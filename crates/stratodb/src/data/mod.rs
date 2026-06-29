//! Persisted data model: the [`Scalar`] leaf type, the [`SValue`] scalar
//! mapping trait, the [`SData`] composite trait, and the accessor machinery.

mod bytes;
mod definition;
mod identifiable;
mod leaf_mut;
mod leaf_ref;
mod map;
mod opt;
mod scalar;
mod seq;
mod smut;
mod sref;
mod value;

#[cfg(any(
    all(not(feature = "bigint-as-scalar"), feature = "bigint-as-data"),
    all(not(feature = "bigfloat-as-scalar"), feature = "bigfloat-as-data"),
    all(not(feature = "rational-as-scalar"), feature = "rational-as-data"),
))]
mod bignum;

pub use self::{
    bytes::Bytes,
    definition::SData,
    map::{Map, MapMut},
    opt::{OptMut, OptRef},
    scalar::Scalar,
    seq::{Seq, SeqMut},
    value::SValue,
};

pub mod leaf {
    pub use super::{leaf_mut::LeafMut, leaf_ref::Leaf};
}

pub mod refs {
    pub use super::{identifiable::SIdentifiable, smut::SMut, sref::SRef};
}
