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
