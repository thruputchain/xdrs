//! RPC client connection.

use std::{net::SocketAddr, sync::Arc};

use tracing::Instrument;

use crate::{ConnectionConfig, ConnectionHandle, tasks::launch_read_task, tcp_connection::connect};

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

    tokio::spawn({
        let client_handle = client_handle.clone();
        async move {
            match launch_read_task(service_map, client_handle.clone(), client_read).await {
                Ok(_) => (),
                Err(e) => {
                    tracing::error!(error = %e, "client read task failed");
                    client_handle.poison().await;
                }
            }
        }
        .instrument(span)
    });

    Ok(client_handle)
}
