//! Zero-copy RPC response wrapper.

/// Response from an RPC call. Wraps the raw record buffer and payload
/// offset, providing zero-copy access to the payload via `Deref<Target = [u8]>`.
///
/// The caller deserializes directly from `&*response` without any copy.
#[derive(Clone)]
pub struct RpcResponse {
    buf: Vec<u8>,
    offset: usize,
}

impl RpcResponse {
    /// Create a response from a payload Vec with no header (offset 0).
    pub fn from_payload(payload: Vec<u8>) -> Self {
        Self {
            buf: payload,
            offset: 0,
        }
    }

    /// Create a response from a raw record buffer and the byte offset where
    /// the payload begins.
    ///
    /// This is driver plumbing, not part of the supported API; use
    /// [`from_payload`](Self::from_payload) to construct responses in tests
    /// and mocks.
    #[doc(hidden)]
    pub fn from_record(buf: Vec<u8>, offset: usize) -> Self {
        debug_assert!(offset <= buf.len());
        Self { buf, offset }
    }
}

impl std::ops::Deref for RpcResponse {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.buf[self.offset..]
    }
}

impl AsRef<[u8]> for RpcResponse {
    fn as_ref(&self) -> &[u8] {
        self
    }
}

impl PartialEq<[u8]> for RpcResponse {
    fn eq(&self, other: &[u8]) -> bool {
        &**self == other
    }
}

impl<const N: usize> PartialEq<[u8; N]> for RpcResponse {
    fn eq(&self, other: &[u8; N]) -> bool {
        &**self == other
    }
}

impl<const N: usize> PartialEq<&[u8; N]> for RpcResponse {
    fn eq(&self, other: &&[u8; N]) -> bool {
        &**self == *other
    }
}

impl PartialEq<&[u8]> for RpcResponse {
    fn eq(&self, other: &&[u8]) -> bool {
        &**self == *other
    }
}

impl PartialEq<Vec<u8>> for RpcResponse {
    fn eq(&self, other: &Vec<u8>) -> bool {
        &**self == other.as_slice()
    }
}

impl PartialEq for RpcResponse {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl Eq for RpcResponse {}

impl std::fmt::Debug for RpcResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcResponse")
            .field("len", &self.len())
            .finish()
    }
}
