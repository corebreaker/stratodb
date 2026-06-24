//! Low-level byte (de)serialization helpers shared by the internal codecs.
//!
//! Multi-byte integers are written big-endian so that fixed-width encodings are
//! directly byte-comparable. Variable-length byte runs are length-prefixed so
//! that the encoders are self-delimiting.

use crate::error::{SdbError, SdbResult};

pub(crate) fn put_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_be_bytes());
}

pub(crate) fn put_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_be_bytes());
}

/// Writes a length-prefixed (u32 big-endian) byte run.
pub(crate) fn put_bytes(buf: &mut Vec<u8>, bytes: &[u8]) {
    put_u32(buf, bytes.len() as u32);
    buf.extend_from_slice(bytes);
}

/// A forward-only cursor over a byte buffer used during decoding.
pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self {
            buf,
            pos: 0,
        }
    }

    fn take(&mut self, n: usize) -> SdbResult<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| SdbError::Corrupt("length overflow while decoding".into()))?;

        if end > self.buf.len() {
            return Err(SdbError::Corrupt("unexpected end of buffer while decoding".into()));
        }

        let slice = &self.buf[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    pub(crate) fn u8(&mut self) -> SdbResult<u8> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn u32(&mut self) -> SdbResult<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub(crate) fn u64(&mut self) -> SdbResult<u64> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }

    /// Reads a fixed-size array of `N` bytes.
    pub(crate) fn array<const N: usize>(&mut self) -> SdbResult<[u8; N]> {
        Ok(self.take(N)?.try_into().unwrap())
    }

    /// Reads a length-prefixed byte run written by [`put_bytes`].
    pub(crate) fn bytes(&mut self) -> SdbResult<&'a [u8]> {
        let n = self.u32()? as usize;
        self.take(n)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.pos >= self.buf.len()
    }
}
