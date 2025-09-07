use std::{net::SocketAddr, sync::Arc};

use tokio::{net::TcpListener, sync::mpsc};

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
            let (read, write) = accept(&self.listener).await?;

            let (tx, rx) = mpsc::unbounded_channel();

            // server write task
            tokio::spawn(async move {
                launch_write_task(rx, write).await.unwrap();
            });

            let service_map = self.service_map.clone();
            // server read task
            tokio::spawn(async move {
                launch_read_task(service_map, Arc::new(ClientHandle::new(tx)), read)
                    .await
                    .unwrap();
            });
        }
    }
}
