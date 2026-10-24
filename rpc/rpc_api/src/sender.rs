//! Trait for making outgoing RPC calls.

use crate::{CallBuf, ProgVers, RpcError, RpcResponse};
use async_trait::async_trait;

/// Trait for making outgoing RPC calls.
///
/// `thruput_rpc_driver` implements this for real TCP connections
/// (`ConnectionHandle`, `ClientConnection`) and for its pooled connections
/// (`FixedSerialPool`, `DynamicPool`, `CpuSetPool`). Inside an
/// [`RpcHandler`](crate::RpcHandler) dispatch, the per-connection sender is
/// available via `thruput_rpc_driver::peer()`.
///
/// Generated protocol clients hold an `Arc<dyn RpcSender>` and are agnostic
/// to which implementation backs it. Implement on a mock struct for unit
/// testing without TCP.
#[async_trait]
pub trait RpcSender: Send + Sync {
    /// A buffer to encode the next call into.
    ///
    /// Senders that recycle record buffers hand back a spent one, so a
    /// steady stream of calls allocates nothing. A sender with nothing to
    /// recycle returns [`CallBuf::new`]. Encode the arguments into the
    /// buffer and pass it to [`send_call`](Self::send_call).
    fn call_buf(&self) -> CallBuf;

    /// Send an RPC call and return the response.
    ///
    /// The call's arguments arrive encoded in a [`CallBuf`], which the
    /// driver frames in place and hands to the socket without copying. The
    /// returned [`RpcResponse`] derefs to `&[u8]` for zero-copy
    /// deserialization, or can be copied to an owned `Vec<u8>` via
    /// `.to_vec()`.
    async fn send_call(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
    ) -> Result<RpcResponse, RpcError>;

    /// Send a call and run `decode` on the reply's payload, which an
    /// implementation may pass straight from its receive buffer.
    ///
    /// `decode` runs at most once, only for a successful reply. The
    /// implementation stops using it before this future returns or finishes
    /// being dropped. The default awaits [`send_call`](Self::send_call) and
    /// decodes the owned response in the caller's task; transports override
    /// it to skip that copy, running `decode` on their reader instead.
    ///
    /// # Safety
    ///
    /// Do not leak this future (`mem::forget`, a leaked box) and then free or
    /// reuse what `decode` borrows. An implementation may call `decode` from
    /// another thread until this future completes or its drop returns, and
    /// leaking skips the drop that stops it. Dropping the future is fine.
    ///
    /// `decode` may run on the connection's reader, so it must not block, and
    /// above all must not wait for another call on the same connection,
    /// whose reply that reader would have to deliver.
    ///
    /// The rest is enforced by the signature: `decode` and what it captures
    /// stay alive and unaliased while borrowed, and the `for<'reply>` bound
    /// stops it from keeping the slice.
    async unsafe fn send_call_decoded(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
        decode: &mut (dyn for<'reply> FnMut(&'reply [u8]) -> Result<(), RpcError> + Send),
    ) -> Result<(), RpcError> {
        let response = self.send_call(progvers, proc, call).await?;
        decode(&response)
    }

    /// Send an RPC call without waiting for the response.
    ///
    /// Default implementation calls [`send_call`](Self::send_call) and
    /// discards the response. Implementations may override this to return
    /// after queueing the request, in which case success does not confirm
    /// socket delivery or peer-side dispatch.
    async fn send_fire_and_forget(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
    ) -> Result<(), RpcError> {
        self.send_call(progvers, proc, call).await.map(|_| ())
    }
}
