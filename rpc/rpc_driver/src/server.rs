//! RPC server implementation.

use std::{net::SocketAddr, sync::Arc};

use tokio::{net::TcpListener, sync::mpsc};

use crate::{
    tasks::{launch_read_task, launch_write_task},
    tcp_connection::accept,
    ClientHandle, ServiceMap,
};

/// TCP-based RPC server that dispatches calls to registered services.
pub struct RpcServer {
    service_map: Arc<ServiceMap>,
    listener: TcpListener,
}

impl RpcServer {
    /// Create a new server with the given services and TCP listener.
    pub fn new(service_map: ServiceMap, listener: TcpListener) -> Arc<Self> {
        Arc::new(Self {
            service_map: Arc::new(service_map),
            listener,
        })
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
    pub async fn run(&self) -> Result<(), std::io::Error> {
        loop {
            let (read, write) = match accept(&self.listener).await {
                Ok((read, write)) => (read, write),
                Err(e) => {
                    eprintln!("Error server accepting connection: {}", e);
                    continue;
                }
            };

            let (tx, rx) = mpsc::unbounded_channel();

            let (client_handle, shutdown_rx) = ClientHandle::new(tx);

            let client_handle = Arc::new(client_handle);
            let client_handle_read = client_handle.clone();
            tokio::spawn(async move {
                match launch_write_task(rx, write, shutdown_rx).await {
                    Ok(_) => (),
                    Err(e) => eprintln!("Error from server write task: {}", e),
                }
                client_handle.poison().await;
            });

            let service_map = self.service_map.clone();
            // server read task
            tokio::spawn(async move {
                match launch_read_task(service_map, client_handle_read.clone(), read).await {
                    Ok(_) => (),
                    Err(e) => {
                        eprintln!("Error from server read task: {}", e);
                        client_handle_read.poison().await;
                    }
                }
            });
        }
    }
}
