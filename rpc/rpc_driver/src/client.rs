use std::{net::SocketAddr, sync::Arc};

use tokio::sync::mpsc;

use crate::{
    tasks::{launch_read_task, launch_write_task},
    tcp_connection::connect,
    ClientHandle, ServiceMap,
};

pub async fn connect_client(addr: &SocketAddr) -> Result<Arc<ClientHandle>, std::io::Error> {
    let (client_read, client_write) = connect(addr).await?;
    let (client_write_tx, client_write_rx) = mpsc::unbounded_channel();

    let client_handle = Arc::new(ClientHandle::new(client_write_tx));

    tokio::spawn(async move {
        launch_write_task(client_write_rx, client_write)
            .await
            .unwrap();
    });
    tokio::spawn({
        let client_handle = client_handle.clone();
        async {
            launch_read_task(Arc::new(ServiceMap::new()), client_handle, client_read)
                .await
                .unwrap();
        }
    });

    Ok(client_handle)
}
