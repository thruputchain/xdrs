use std::{net::SocketAddr, sync::Arc};

use tokio::{
    net::TcpListener,
    sync::{mpsc, oneshot},
};

use crate::{
    tasks::{launch_read_task, launch_write_task},
    tcp_connection::accept,
    ClientHandle, ServiceMap,
};

pub struct RpcServer {
    service_map: Arc<ServiceMap>,
    listener: TcpListener,
}

impl RpcServer {
    pub fn new(service_map: ServiceMap, listener: TcpListener) -> Arc<Self> {
        Arc::new(Self {
            service_map: Arc::new(service_map),
            listener,
        })
    }

    pub fn addr(&self) -> Result<SocketAddr, std::io::Error> {
        self.listener.local_addr()
    }

    pub async fn start(self: Arc<Self>) -> tokio::task::JoinHandle<Result<(), std::io::Error>> {
        let x = self.clone();
        tokio::spawn(async move { x.run().await })
    }

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

            // Create a shutdown signal for the server write task (never fires, but required by API)
            // We need to keep the sender alive, otherwise the receiver will complete immediately
            let (shutdown_tx, shutdown_rx) = oneshot::channel();

            // server write task
            let shutdown_tx_handle = shutdown_tx; // Keep sender alive
            tokio::spawn(async move {
                let _shutdown_tx = shutdown_tx_handle; // Keep sender alive for the lifetime of the task
                match launch_write_task(rx, write, shutdown_rx).await {
                    Ok(_) => (),
                    Err(e) => eprintln!("Error from server write task: {}", e),
                }
            });

            let service_map = self.service_map.clone();
            // server read task
            tokio::spawn(async move {
                let (client_handle, _shutdown_rx) = ClientHandle::new(tx);
                let client_handle = Arc::new(client_handle);
                match launch_read_task(service_map, client_handle.clone(), read).await {
                    Ok(_) => (),
                    Err(e) => {
                        eprintln!("Error from server read task: {}", e);
                        client_handle.poison().await;
                    }
                }
            });
        }
    }
}
