//! The [`SData`] composite trait.

/// Composite types (structs and enums) that decompose into a node subtree.
///
/// Implemented automatically by `#[derive(SData)]`. The derive macro and the
/// trait's methods are added in a later milestone; for now this anchors the
/// public name.
pub trait SData {}
