//! Outgoing call payload with the wire prefix reserved in front of it.

use std::fmt;

/// Payload of an outgoing RPC call, with room reserved ahead of it for the
/// record mark and call header the driver writes when it sends the call.
///
/// Encoding arguments straight into a `CallBuf` lets the driver frame the
/// call in place: it fills the reserved prefix and hands the buffer to the
/// socket without copying or shifting the payload. Only the payload is
/// reachable through this type, so nothing a caller can do touches the
/// prefix. [`encode`](Self::encode) serializes XDR values into the payload.
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

    /// Serialize `value` as XDR onto the end of the payload.
    ///
    /// Generated clients encode their argument tuple this way, so the driver
    /// can send the buffer without copying it.
    pub fn encode<T>(&mut self, value: &T) -> xdrs::WriteResult<()>
    where
        T: xdrs::WriteXDR,
    {
        let mut writer = xdrs::txs::buf_writer::BufWriter::from(std::mem::take(&mut self.buf));
        let result = xdrs::write_xdr(value, &mut writer);
        self.buf = writer.into();
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
    fn encode_appends_xdr_after_prefix() {
        let mut buf = call_with(b"pre");
        buf.encode(&7u32).unwrap();
        buf.encode(&(1u16, true)).unwrap();
        assert_eq!(
            buf.payload(),
            [
                b"pre".as_slice(),
                &xdrs::txs::encode(&(7u32, (1u16, true))).unwrap()
            ]
            .concat()
        );
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
