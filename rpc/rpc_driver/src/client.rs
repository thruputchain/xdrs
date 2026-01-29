//! RPC client connection.

use std::{net::SocketAddr, sync::Arc};

use tokio::sync::mpsc;

use crate::{
    ClientHandle, ServiceMap,
    tasks::{launch_read_task, launch_write_task},
    tcp_connection::connect,
};

/// Configuration options for the RPC client.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Maximum size of a single RPC message in bytes.
    /// If not set, defaults to 16MB (0x100_0000).
    pub max_message_size: Option<u32>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            max_message_size: None,
        }
    }
}

/// Connect to an RPC server and return a handle for making calls.
///
/// Spawns background tasks for reading and writing on the connection.
pub async fn connect_client(addr: &SocketAddr) -> Result<Arc<ClientHandle>, std::io::Error> {
    connect_client_with_config(addr, ClientConfig::default()).await
}

/// Connect to an RPC server with custom configuration.
///
/// Spawns background tasks for reading and writing on the connection.
pub async fn connect_client_with_config(
    addr: &SocketAddr,
    config: ClientConfig,
) -> Result<Arc<ClientHandle>, std::io::Error> {
    let (client_read, client_write) = connect(addr, config.max_message_size).await?;
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
            match launch_read_task(
                Arc::new(ServiceMap::new()),
                client_handle.clone(),
                client_read,
            )
            .await
            {
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
