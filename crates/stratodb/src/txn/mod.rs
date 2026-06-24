//! Opaque read and write transactions.

mod read;
mod write;

pub use self::{read::ReadTxn, write::WriteTxn};
