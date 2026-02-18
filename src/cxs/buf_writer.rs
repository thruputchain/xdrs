//! CXS buffer writer implementation.
//!
//! This module provides [`BufWriter`], which implements [`Writer`]
//! for writing CXS-encoded XDR data to a growable buffer.

use super::put::*;
use crate::serialize::writer::*;

use alloc::{vec, vec::Vec};

/// Write CXS-encoded XDR data into a growable buffer.
///
/// This writer implements the [`Writer`] trait for the CXS (Compact XDR
/// Serialization) format. It writes data to an internal `Vec<u8>` buffer that
/// grows as needed.
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
pub struct BufWriter(Vec<u8>);

impl BufWriter {
    /// Creates a new `BufWriter` with an empty buffer.
    pub fn new() -> Self {
        Self(vec![])
    }

    /// Clear the buffer, to later reuse the memory
    /// for another encoding
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Reserve space for the given number of bytes
    pub fn reserve(&mut self, len: usize) {
        self.0.reserve(len);
    }

    /// Returns the length of the buffer in bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` if the buffer contains no bytes.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Creates a new `BufWriter` with the specified capacity.
    pub fn with_capacity(cap: usize) -> Self {
        Self(Vec::with_capacity(cap))
    }
}

impl Default for BufWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Vec<u8>> for BufWriter {
    fn from(vec: Vec<u8>) -> Self {
        Self(vec)
    }
}

impl AsRef<[u8]> for BufWriter {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl AsMut<[u8]> for BufWriter {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}

impl From<BufWriter> for Vec<u8> {
    fn from(w: BufWriter) -> Self {
        w.0
    }
}

// Implement the basic byte operations in order
// to get a Writer implementation for free
impl Put for BufWriter {
    fn put_byte(&mut self, b: u8) -> WriteResult<()> {
        self.0.push(b);
        Ok(())
    }

    fn put_bytes(&mut self, bytes: &[u8]) -> WriteResult<()> {
        if bytes.len() > u32::MAX as usize {
            return Err(WriteError::InvalidArrayLength);
        }

        self.0.extend_from_slice(bytes);

        Ok(())
    }
}
