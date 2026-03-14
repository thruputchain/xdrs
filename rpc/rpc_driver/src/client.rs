//! RPC client connection.

use std::{net::SocketAddr, sync::Arc};

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
    let (client_read, client_write) = connect(addr, config.max_message_size).await?;
    let service_map = config.services.clone();

    let client_handle = Arc::new(ConnectionHandle::new(client_write, &config));

    tokio::spawn({
        let client_handle = client_handle.clone();
        async move {
            match launch_read_task(service_map, client_handle.clone(), client_read).await {
                Ok(_) => (),
                Err(e) => {
                    eprintln!("Error from client read task: {}", e);
                    client_handle.poison().await;
                }
            }
        }
    });

    Ok(client_handle)
}
