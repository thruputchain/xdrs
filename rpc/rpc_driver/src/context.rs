//! Per-connection context available inside RPC handlers via task-local storage.

use std::sync::Arc;

use crate::connection::RpcSender;

tokio::task_local! {
    static RPC_PEER: Arc<dyn RpcSender>;
}

/// Get the peer sender for the current connection.
///
/// Only valid inside an [`RpcHandler::dispatch`](crate::handler::RpcHandler::dispatch) call.
/// Panics if called outside a dispatch context.
///
/// # Callbacks to the peer
///
/// **With `concurrent-dispatch` feature:** Calling back to the peer inline
/// works — the read loop processes replies concurrently with active handlers.
///
/// **Without `concurrent-dispatch` (default):** Calling back within `dispatch`
/// will deadlock because the read loop awaits `dispatch` inline. Instead,
/// capture the sender and spawn a separate task:
///
/// ```ignore
/// let peer = rpc_driver::peer();  // capture inside dispatch
/// tokio::spawn(async move {
///     peer.send_call(prog, 1, &data).await.unwrap();
/// });
/// ```
///
/// You **must** call `peer()` before `tokio::spawn` — task-locals do not
/// propagate into spawned tasks, so calling `peer()` inside one will panic.
pub fn peer() -> Arc<dyn RpcSender> {
    RPC_PEER.with(|s| s.clone())
}

/// Try to get the peer sender, returning `None` if called outside a dispatch context.
pub fn try_peer() -> Option<Arc<dyn RpcSender>> {
    RPC_PEER.try_with(|s| s.clone()).ok()
}

/// Run a future with the given peer sender set in the task-local context.
///
/// This is used by the framework's dispatch loop — you should not need to call
/// this directly unless you are writing custom dispatch infrastructure.
pub(crate) async fn with_peer<F, R>(sender: Arc<dyn RpcSender>, f: F) -> R
where
    F: std::future::Future<Output = R>,
{
    RPC_PEER.scope(sender, f).await
}
