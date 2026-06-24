//! Low-level byte (de)serialization helpers shared by the internal codecs.
//!
//! Multi-byte integers are written big-endian so that fixed-width encodings are
//! directly byte-comparable. Variable-length byte runs are length-prefixed so
//! that the encoders are self-delimiting.

mod putters;
mod reader;

pub(crate) use self::{
    putters::{put_u32, put_bytes},
    reader::Reader,
};
