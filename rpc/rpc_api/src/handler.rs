//! Trait for implementing RPC service handlers.

use crate::{ProgVers, RpcError};
use async_trait::async_trait;

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
/// With the driver's default serial dispatch, calling back to the peer
/// inline will deadlock — spawn a separate task instead. With the driver's
/// `concurrent-dispatch` feature enabled, inline callbacks work without
/// deadlocking.
#[async_trait]
pub trait RpcHandler: Send + Sync {
    /// Handle an incoming RPC call for the given procedure number.
    async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError>;

    /// Return the program number and version this service handles.
    fn prog_vers(&self) -> ProgVers;
}
