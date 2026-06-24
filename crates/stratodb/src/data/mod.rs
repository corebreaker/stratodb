//! Persisted data model: the [`Scalar`] leaf type, the [`SValue`] scalar
//! mapping trait, and the [`SData`] composite trait.

mod definition;
mod scalar;
mod value;

pub use self::{definition::SData, scalar::Scalar, value::SValue};
