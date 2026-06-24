//! Persisted data model: the [`Scalar`] leaf type, the [`SValue`] scalar
//! mapping trait, the [`SData`] composite trait, and the accessor machinery.

mod definition;
mod identifiable;
mod leaf_mut;
mod leaf_ref;
mod scalar;
mod smut;
mod sref;
mod value;

pub use self::{definition::SData, scalar::Scalar, value::SValue};

pub mod leaf {
    pub use super::{leaf_ref::Leaf, leaf_mut::LeafMut};
}

pub mod refs {
    pub use super::{sref::SRef, smut::SMut, identifiable::SIdentifiable};
}
