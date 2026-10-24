//! Trait for implementing RPC service handlers.

use crate::{ProgVers, ReplyBuf, RpcError};
use std::future::Future;
use std::pin::Pin;

/// A handler's invocation for one call: decoded arguments in, encoded reply
/// out. It may borrow its service but not the request bytes.
pub type RpcFuture<'a> = Pin<Box<dyn Future<Output = Result<ReplyBuf, RpcError>> + Send + 'a>>;

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
    /// and returns it, and return that block boxed. `reply` comes from the
    /// driver's buffer cache with the reply header reserved. Neither the
    /// driver nor the returned future needs to keep a copy of the serialized
    /// request.
    ///
    /// Invalid procedures and malformed arguments can be rejected immediately.
    ///
    /// A future borrowing the input is rejected at compile time:
    ///
    /// ```compile_fail
    /// use thruput_rpc_api::{ProgVers, ReplyBuf, RpcError, RpcFuture, RpcHandler};
    /// struct BorrowingHandler;
    /// impl RpcHandler for BorrowingHandler {
    ///     fn prog_vers(&self) -> ProgVers { ProgVers::new(1, 1) }
    ///     fn dispatch(
    ///         &self,
    ///         _: u32,
    ///         data: &[u8],
    ///         reply: ReplyBuf,
    ///     ) -> Result<RpcFuture<'_>, RpcError> {
    ///         Ok(Box::pin(async move { Ok(reply.with(data)) }))
    ///     }
    /// }
    /// ```
    fn dispatch(&self, proc: u32, data: &[u8], reply: ReplyBuf) -> Result<RpcFuture<'_>, RpcError>;

    /// Return the program number and version this service handles.
    fn prog_vers(&self) -> ProgVers;
}
