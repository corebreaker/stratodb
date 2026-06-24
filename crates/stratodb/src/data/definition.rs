//! The [`SData`] composite trait.

use super::refs::{SRef, SMut};
use crate::{
    access::{Reader, Writer},
    error::SdbResult,
    path::SPath,
};

/// Composite types (structs and enums) that decompose into a node subtree.
///
/// Implemented automatically by `#[derive(SData)]`. Scalars, `Vec`, `Option` and
/// maps implement it too, so decomposition is uniform: every field is stored and
/// loaded through this same trait.
pub trait SData: Sized {
    /// Read accessor produced by `ReadTxn::fetch` (a `StratoXXX` or [`super::Leaf`]).
    type Ref<'t>: SRef<'t>;

    /// Write accessor produced by `WriteTxn::fetch_mut` (a `StratoXXXMut` or
    /// [`super::LeafMut`]).
    type Mut<'t>: SMut<'t>;

    /// Decomposes `self` into the subtree rooted at `at`.
    fn store<W: Writer>(&self, writer: &W, at: &SPath) -> SdbResult<()>;

    /// Recomposes a value from the subtree rooted at `at`.
    fn load<R: Reader>(reader: &R, at: &SPath) -> SdbResult<Self>;
}
