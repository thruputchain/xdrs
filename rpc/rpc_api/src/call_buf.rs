//! Outgoing call payload with the wire prefix reserved in front of it.

use std::fmt;

/// Payload of an outgoing RPC call, with room reserved ahead of it for the
/// record mark and call header the driver writes when it sends the call.
///
/// Encoding arguments straight into a `CallBuf` lets the driver frame the
/// call in place: it fills the reserved prefix and hands the buffer to the
/// socket without copying or shifting the payload. Callers read only the
/// payload; [`append_with`](Self::append_with) hands a serializer the
/// record buffer to extend in place, with the prefix kept reserved.
pub struct CallBuf {
    /// `buf[..PREFIX_LEN]` is reserved; `buf[PREFIX_LEN..]` is the payload.
    buf: Vec<u8>,
}

impl CallBuf {
    /// Bytes reserved ahead of the payload: a 4-byte record mark and the
    /// 40-byte RFC 5531 CALL header with `AUTH_NONE` credentials and
    /// verifier. The driver only ever sends that header, so this is a
    /// constant of the wire format.
    pub const PREFIX_LEN: usize = 4 + 40;

    /// Buffer with an empty payload.
    ///
    /// Prefer `RpcSender::call_buf`, which recycles a spent buffer instead
    /// of allocating. There is deliberately no `Default`: a buffer should
    /// come from a sender unless there is none.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self::from_spare(Vec::new())
    }

    /// Reuse a spent buffer's allocation: its contents are discarded and the
    /// prefix re-reserved.
    ///
    /// This is driver plumbing for recycling record buffers, not part of the
    /// supported API; callers get recycled buffers through
    /// `RpcSender::call_buf`.
    #[doc(hidden)]
    pub fn from_spare(mut spare: Vec<u8>) -> Self {
        spare.clear();
        spare.resize(Self::PREFIX_LEN, 0);
        Self { buf: spare }
    }

    /// The complete record buffer, reserved prefix first.
    ///
    /// This is driver plumbing for framing the call, not part of the
    /// supported API.
    #[doc(hidden)]
    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }

    /// Append to the payload through the record buffer itself.
    ///
    /// `f` receives the whole record `Vec<u8>`, the reserved prefix followed
    /// by the payload so far, appends to it, and its return value is passed
    /// back. Generated protocol clients serialize their argument tuple this
    /// way, straight into the buffer with the XDR serializer they were
    /// generated against, so the driver sends the record without copying it
    /// and this crate carries no serializer dependency of its own.
    ///
    /// `f` must only append. The buffer is kept at least
    /// [`PREFIX_LEN`](Self::PREFIX_LEN) long afterwards, so an `f` that
    /// shortens it cannot unreserve the prefix; the driver overwrites the
    /// prefix bytes when it frames the call, so their contents never matter.
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
}

impl fmt::Debug for CallBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CallBuf")
            .field("payload_len", &self.payload().len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call_with(payload: &[u8]) -> CallBuf {
        let mut call = CallBuf::new();
        call.extend_from_slice(payload);
        call
    }

    #[test]
    fn new_has_empty_payload_and_full_prefix() {
        let buf = CallBuf::new();
        assert!(buf.payload().is_empty());
        assert_eq!(buf.into_vec().len(), CallBuf::PREFIX_LEN);
    }

    #[test]
    fn payload_sits_after_prefix() {
        let mut buf = call_with(b"abc");
        buf.extend_from_slice(b"def");
        assert_eq!(buf.payload(), b"abcdef");

        let vec = buf.into_vec();
        assert_eq!(vec.len(), CallBuf::PREFIX_LEN + 6);
        assert_eq!(&vec[CallBuf::PREFIX_LEN..], b"abcdef");
    }

    #[test]
    fn append_with_appends_after_prefix_and_returns_value() {
        let mut buf = call_with(b"pre");
        let len = buf.append_with(|vec| {
            vec.extend_from_slice(b"fix");
            vec.len()
        });
        assert_eq!(len, CallBuf::PREFIX_LEN + 6);
        assert_eq!(buf.payload(), b"prefix");
    }

    #[test]
    fn append_with_keeps_prefix_reserved() {
        let mut buf = call_with(b"abc");
        buf.append_with(Vec::clear);
        assert!(buf.payload().is_empty());
        assert_eq!(buf.into_vec().len(), CallBuf::PREFIX_LEN);
    }

    #[test]
    fn from_spare_reuses_allocation_and_reserves_prefix() {
        let spare = Vec::with_capacity(1024);
        let ptr = spare.as_ptr();
        let mut buf = CallBuf::from_spare(spare);
        assert!(buf.payload().is_empty());
        buf.extend_from_slice(&[1; 512]);
        let vec = buf.into_vec();
        assert_eq!(vec.as_ptr(), ptr);
        assert_eq!(vec.len(), CallBuf::PREFIX_LEN + 512);

        let dirty = vec![7; 100];
        assert!(CallBuf::from_spare(dirty).payload().is_empty());
    }
}
