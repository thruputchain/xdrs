//! Outgoing reply payload with the wire prefix reserved in front of it.

use std::fmt;

/// Payload of a successful RPC reply, with room reserved ahead of it for the
/// record mark and reply header the driver writes when it sends the reply.
///
/// The driver hands each call a `ReplyBuf` taken from its buffer cache.
/// Encoding results straight into it lets the driver frame the reply in
/// place: it fills the reserved prefix and hands the buffer to the socket
/// without allocating or copying a separate result buffer.
pub struct ReplyBuf {
    /// `buf[..PREFIX_LEN]` is reserved; `buf[PREFIX_LEN..]` is the payload.
    buf: Vec<u8>,
}

impl ReplyBuf {
    /// Bytes reserved ahead of the payload: a 4-byte record mark and the
    /// 24-byte RFC 5531 accepted-SUCCESS reply header with an `AUTH_NONE`
    /// verifier. The driver only ever sends that header for a successful
    /// reply, so this is a constant of the wire format.
    pub const PREFIX_LEN: usize = 4 + 24;

    /// Buffer with an empty payload.
    ///
    /// Handlers normally fill the buffer the driver passes to
    /// `RpcHandler::dispatch` instead of allocating one.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self::from_spare(Vec::new())
    }

    /// Reuse a spent buffer's allocation: its contents are discarded and the
    /// prefix re-reserved.
    ///
    /// This is driver plumbing for recycling record buffers, not part of the
    /// supported API.
    #[doc(hidden)]
    pub fn from_spare(mut spare: Vec<u8>) -> Self {
        spare.clear();
        spare.resize(Self::PREFIX_LEN, 0);
        Self { buf: spare }
    }

    /// The complete record buffer, reserved prefix first.
    ///
    /// This is driver plumbing for framing the reply, not part of the
    /// supported API.
    #[doc(hidden)]
    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }

    /// Append to the payload through the record buffer itself.
    ///
    /// `f` receives the whole record `Vec<u8>`, the reserved prefix followed
    /// by the payload so far, appends to it, and its return value is passed
    /// back. Generated dispatchers serialize their results this way.
    ///
    /// `f` must only append. The buffer is kept at least
    /// [`PREFIX_LEN`](Self::PREFIX_LEN) long afterwards; the driver overwrites
    /// the prefix bytes when it frames the reply.
    pub fn append_with<R>(&mut self, f: impl FnOnce(&mut Vec<u8>) -> R) -> R {
        let result = f(&mut self.buf);
        if self.buf.len() < Self::PREFIX_LEN {
            self.buf.resize(Self::PREFIX_LEN, 0);
        }
        result
    }

    /// The payload bytes.
    pub fn payload(&self) -> &[u8] {
        &self.buf[Self::PREFIX_LEN..]
    }

    /// Append `bytes` to the payload.
    pub fn extend_from_slice(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// This buffer with `bytes` appended to the payload.
    pub fn with(mut self, bytes: &[u8]) -> Self {
        self.extend_from_slice(bytes);
        self
    }
}

impl fmt::Debug for ReplyBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReplyBuf")
            .field("payload_len", &self.payload().len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_sits_after_prefix() {
        let buf = ReplyBuf::new().with(b"abc").with(b"def");
        assert_eq!(buf.payload(), b"abcdef");
        let vec = buf.into_vec();
        assert_eq!(vec.len(), ReplyBuf::PREFIX_LEN + 6);
        assert_eq!(&vec[ReplyBuf::PREFIX_LEN..], b"abcdef");
    }

    #[test]
    fn append_with_keeps_prefix_reserved() {
        let mut buf = ReplyBuf::new().with(b"abc");
        buf.append_with(Vec::clear);
        assert!(buf.payload().is_empty());
        assert_eq!(buf.into_vec().len(), ReplyBuf::PREFIX_LEN);
    }

    #[test]
    fn from_spare_reuses_allocation_and_reserves_prefix() {
        let spare = Vec::with_capacity(1024);
        let ptr = spare.as_ptr();
        let vec = ReplyBuf::from_spare(spare).with(&[1; 512]).into_vec();
        assert_eq!(vec.as_ptr(), ptr);
        assert_eq!(vec.len(), ReplyBuf::PREFIX_LEN + 512);
        assert!(ReplyBuf::from_spare(vec![7; 100]).payload().is_empty());
    }
}
