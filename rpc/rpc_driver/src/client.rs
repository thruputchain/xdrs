//! RPC client connection.

use std::{net::SocketAddr, sync::Arc};

use crate::{
    ConnectionConfig, ConnectionHandle, tasks::spawn_client_read_task, tcp_connection::connect,
};

fn attach_client_read_runner(
    handle: &Arc<ConnectionHandle>,
    runner: crate::tasks::ClientReadRunner,
) {
    handle.attach_client_read_runner(runner);
}

/// Connect to an RPC server and return a handle for making calls.
///
/// Spawns a background read task for the connection.
pub async fn connect_client(addr: &SocketAddr) -> Result<Arc<ConnectionHandle>, std::io::Error> {
    connect_client_with_config(addr, ConnectionConfig::default()).await
}

/// Connect to an RPC server with custom configuration.
///
/// Spawns a background read task for the connection.
pub async fn connect_client_with_config(
    addr: &SocketAddr,
    config: ConnectionConfig,
) -> Result<Arc<ConnectionHandle>, std::io::Error> {
    let (client_read, client_write) =
        connect(addr, config.max_message_size, config.read_buffer_size).await?;
    let service_map = config.services.clone();

    let span = tracing::info_span!("rpc_connection", peer = %addr, role = "client");
    let client_handle = Arc::new(ConnectionHandle::new(client_write, &config, span.clone()));
    let runner = spawn_client_read_task(service_map, client_handle.clone(), client_read, span);
    attach_client_read_runner(&client_handle, runner);

    Ok(client_handle)
}
