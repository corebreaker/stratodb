//! Cursors and accessor traits shared by every generated `StratoXXX` type.
//!
//! A cursor is an opaque, copyable handle bound to a transaction; an accessor
//! pairs a cursor with the path and primary key of the node it points at. Read
//! accessors implement [`SRef`](crate::data::refs::SRef), write accessors
//! [`SMut`](crate::data::refs::SMut); both expose the
//! node's primary key via `key()` (resolved eagerly when the accessor is built).

mod bound;
mod reader;
mod rooted;
mod writer;

pub(crate) use self::{bound::BoundCursor, reader::ReadCursor, rooted::Rooted, writer::WriteCursor};

pub use self::{reader::Reader, writer::Writer};
