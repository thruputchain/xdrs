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
/// Calling back to the peer inline works: a suspended handler runs in its
/// own task while the reader continues routing replies. If you spawn an
/// additional task yourself, capture its peer sender first. The
/// [`RpcHandler`](crate::handler::RpcHandler) docs published with
/// `thruput_rpc_api` 0.2.0 predate this and still describe a serial dispatch mode
/// and a `concurrent-dispatch` feature; neither exists, and this crate's
/// behavior is the one that applies.
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
#[allow(unreachable_pub)]
pub fn with_peer<F, R>(sender: Arc<dyn RpcSender>, f: F) -> impl std::future::Future<Output = R>
where
    F: std::future::Future<Output = R>,
{
    // Construct the scope eagerly so even a future dropped before its first
    // poll drops `f` with its peer installed.
    RPC_PEER.scope(sender, f)
}
