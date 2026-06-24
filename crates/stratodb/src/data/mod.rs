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
