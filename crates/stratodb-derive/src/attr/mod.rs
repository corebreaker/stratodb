//! Parsing of the Serde-style `#[strato(...)]` attributes (container and field
//! level) shared by the struct and enum code generators.

mod container;
mod field;
mod rename;

pub(crate) use self::{container::ContainerAttrs, field::FieldAttrs};
