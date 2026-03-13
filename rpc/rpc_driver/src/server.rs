//! RPC server implementation.

use std::{net::SocketAddr, sync::Arc};

use tokio::net::TcpListener;

use crate::{
    ConnectionConfig, ConnectionHandle, ServiceMap, tasks::launch_read_task, tcp_connection::accept,
};

/// TCP-based RPC server that dispatches calls to registered services.
pub struct RpcServer {
    listener: TcpListener,
    config: ConnectionConfig,
}

impl RpcServer {
    /// Create a new server with the given services and TCP listener.
    pub fn new(listener: TcpListener, services: ServiceMap) -> Arc<Self> {
        Self::new_with_config(listener, ConnectionConfig::new(None, services))
    }

    /// Create a new server with custom configuration.
    pub fn new_with_config(listener: TcpListener, config: ConnectionConfig) -> Arc<Self> {
        Arc::new(Self { config, listener })
    }

    /// Returns the local address the server is bound to.
    pub fn addr(&self) -> Result<SocketAddr, std::io::Error> {
        self.listener.local_addr()
    }

    /// Start the server in a background task, returning a handle to it.
    pub async fn start(self: Arc<Self>) -> tokio::task::JoinHandle<Result<(), std::io::Error>> {
        let x = self.clone();
        tokio::spawn(async move { x.run().await })
    }

    /// Run the server, accepting connections and handling requests.
    async fn run(&self) -> Result<(), std::io::Error> {
        loop {
            let (read, write) = match accept(&self.listener, self.config.max_message_size).await {
                Ok((read, write)) => (read, write),
                Err(e) => {
                    eprintln!("Error server accepting connection: {}", e);
                    continue;
                }
            };

            let client_handle = Arc::new(ConnectionHandle::new(write, self.config.log_timestamps));
            let service_map = self.config.services.clone();

            tokio::spawn({
                let client_handle = client_handle.clone();
                async move {
                    match launch_read_task(service_map, client_handle.clone(), read).await {
                        Ok(_) => (),
                        Err(e) => {
                            eprintln!("Error from server read task: {}", e);
                            client_handle.poison().await;
                        }
                    }
                }
            });
        }
    }
}
