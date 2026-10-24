//! Trait for implementing RPC service handlers.

use crate::{FutureSlot, ProgVers, ReplyBuf, RpcError, RpcFuture};

/// Trait for implementing RPC service handlers.
///
/// Implement this trait to define how your service handles incoming RPC
/// calls. Generated protocol dispatchers implement it; hand-written services
/// can too. Register handlers with `thruput_rpc_driver::ServiceMap` and pass the map
/// to `thruput_rpc_driver::RpcServer`.
///
/// For bidirectional RPC (calling back to the peer from within a handler),
/// use `thruput_rpc_driver::peer()` to obtain the per-connection sender.
///
/// The driver polls ready handlers inline and runs suspended handlers in
/// separate tasks, allowing inline callbacks to the peer.
pub trait RpcHandler: Send + Sync + 'static {
    /// Decode an incoming call synchronously, then return its invocation.
    ///
    /// Decode `data` into owned arguments before constructing the future.
    /// Move those arguments and `reply` into an `async move` block that
    /// invokes the service through `self`, encodes its result into `reply`
    /// and returns it, and store that block with `slot.put`. `reply` comes
    /// from the driver's buffer cache with the reply header reserved, and
    /// `slot` is storage the driver reuses across calls, so a call that
    /// completes without suspending allocates nothing. Neither the driver
    /// nor the returned future needs to keep a copy of the serialized
    /// request.
    ///
    /// Invalid procedures and malformed arguments can be rejected immediately.
    ///
    /// A future borrowing the input is rejected at compile time:
    ///
    /// ```compile_fail
    /// use thruput_rpc_api::{FutureSlot, ProgVers, ReplyBuf, RpcError, RpcFuture, RpcHandler};
    /// struct BorrowingHandler;
    /// impl RpcHandler for BorrowingHandler {
    ///     fn prog_vers(&self) -> ProgVers { ProgVers::new(1, 1) }
    ///     fn dispatch(
    ///         &self,
    ///         _: u32,
    ///         data: &[u8],
    ///         reply: ReplyBuf,
    ///         slot: FutureSlot,
    ///     ) -> Result<RpcFuture<'_>, RpcError> {
    ///         Ok(slot.put(async move { Ok(reply.with(data)) }))
    ///     }
    /// }
    /// ```
    fn dispatch(
        &self,
        proc: u32,
        data: &[u8],
        reply: ReplyBuf,
        slot: FutureSlot,
    ) -> Result<RpcFuture<'_>, RpcError>;

    /// Return the program number and version this service handles.
    fn prog_vers(&self) -> ProgVers;
}
