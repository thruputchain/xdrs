//! RPC client connection.

use std::{net::SocketAddr, sync::Arc};

use tokio::sync::mpsc;

use crate::{
    ClientHandle, ConnectionConfig,
    tasks::{launch_read_task, launch_write_task},
    tcp_connection::connect,
};

/// Connect to an RPC server and return a handle for making calls.
///
/// Spawns background tasks for reading and writing on the connection.
pub async fn connect_client(addr: &SocketAddr) -> Result<Arc<ClientHandle>, std::io::Error> {
    connect_client_with_config(addr, ConnectionConfig::default()).await
}

/// Connect to an RPC server with custom configuration.
///
/// Spawns background tasks for reading and writing on the connection.
pub async fn connect_client_with_config(
    addr: &SocketAddr,
    config: ConnectionConfig,
) -> Result<Arc<ClientHandle>, std::io::Error> {
    let (client_read, client_write) = connect(addr, config.max_message_size).await?;
    let service_map = config.services.clone();
    let (client_write_tx, client_write_rx) = mpsc::unbounded_channel();

    let (client_handle, shutdown_rx) = ClientHandle::new(client_write_tx);
    let client_handle = Arc::new(client_handle);

    tokio::spawn({
        let client_handle = client_handle.clone();
        async move {
            match launch_write_task(client_write_rx, client_write, shutdown_rx).await {
                Ok(_) => (),
                Err(e) => {
                    eprintln!("Error from client write task: {}", e);
                    client_handle.poison().await;
                }
            }
        }
    });
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
