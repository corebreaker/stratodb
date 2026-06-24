//! Strato-paths: slash-separated addresses into the node tree.
//!
//! A path is a sequence of [`Segment`]s. Object fields are named (`a/b`); list
//! elements are indexed (`a/t[5]`). Indices bind to the preceding name without a
//! separator, so `a/t[5]/x` parses as `a`, `t`, `[5]`, `x`. A path is resolved by
//! walking the node tree (see [`crate::tree`]); it is never persisted, so it has
//! no byte encoding.
//!
//! Two segments are reserved and normalized away at parse time (a path never
//! stores them): `.` (current) is dropped and `..` (parent) removes the preceding
//! segment, so `a/b/../c` parses as `a/c`. A `..` that would rise above the root
//! is an error — there are no symlinks, so this textual normalization is exact.
//! The `/` operator appends to a path: another path joins segment-wise (`a / b`),
//! a string adds one field name (`a / "x"`). See [`SPath::join`] / [`PathTail`].

mod functions;
mod segment;
mod spath;
mod tail;

pub use self::{segment::Segment, spath::SPath, tail::PathTail};
