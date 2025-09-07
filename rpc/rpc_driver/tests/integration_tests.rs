use rpc_driver::{
    client_state::ClientHandle,
    server_state::ServiceMap,
    tasks::{launch_read_task, launch_write_task},
    tcp_connection::connect,
};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use rpc_driver::{
        rpc_error::RpcError, server::RpcServer, server_state::ServerDispatch,
        tcp_connection::accept, ProgVers,
    };

    use super::*;
    struct EchoCallbackSerivce {}

    #[async_trait]
    impl ServerDispatch for EchoCallbackSerivce {
        async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
            if proc != 1 {
                return Err(RpcError::InvalidProcedure);
            }
            println!(
                "EchoCallbackSerivce: dispatch: proc={}, data={:?}",
                proc, data
            );
            Ok(data.to_vec())
        }

        fn prog_vers(&self) -> ProgVers {
            ProgVers::new(1, 1)
        }
    }

    #[tokio::test]
    async fn test_null_callback() {
        // Set up TCP listener for self-connection
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Create service map
        let mut service_map = ServiceMap::new();
        service_map.add_service(Box::new(EchoCallbackSerivce {}));

        // Set up write channel
        let (client_write_tx, client_write_rx) = mpsc::unbounded_channel();
        let client_handle = Arc::new(ClientHandle::new(client_write_tx));
        let (server_write_tx, server_write_rx) = mpsc::unbounded_channel();
        let server_handle = Arc::new(ClientHandle::new(server_write_tx));

        // Accept incoming connection in background task - we'll use this for the server side
        let listener_handle = tokio::spawn(async move { accept(&listener).await.unwrap() });

        // Connect from client side
        let (client_read, client_write) = connect(addr).await.unwrap();
        let (server_read, server_write) = listener_handle.await.unwrap();

        let client_write_task = tokio::spawn(async move {
            launch_write_task(client_write_rx, client_write)
                .await
                .unwrap();
        });
        let client_read_task = tokio::spawn({
            let client_handle = client_handle.clone();
            async move {
                launch_read_task(Arc::new(ServiceMap::new()), client_handle, client_read)
                    .await
                    .unwrap();
            }
        });

        let server_write_task = tokio::spawn(async move {
            launch_write_task(server_write_rx, server_write)
                .await
                .unwrap();
        });

        let server_read_task = tokio::spawn(async move {
            launch_read_task(Arc::new(service_map), server_handle.clone(), server_read)
                .await
                .unwrap();
        });

        // Give tasks a moment to start up
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let response = client_handle
            .as_ref()
            .send_call(ProgVers::new(1, 1), 1, b"hello")
            .await
            .unwrap();
        assert_eq!(response, b"hello");
        assert_eq!(
            client_handle
                .as_ref()
                .client_state
                .read()
                .await
                .pending_requests
                .len(),
            0
        );

        // some error cases
        assert_eq!(
            client_handle
                .as_ref()
                .send_call(ProgVers::new(1, 1), 2, b"world")
                .await
                .unwrap_err(),
            RpcError::InvalidProcedure
        );
        assert_eq!(
            client_handle
                .as_ref()
                .send_call(ProgVers::new(2, 1), 1, b"world2")
                .await
                .unwrap_err(),
            RpcError::ProgUnavail
        );
        assert_eq!(
            client_handle
                .as_ref()
                .send_call(ProgVers::new(1, 2), 1, b"world3")
                .await
                .unwrap_err(),
            RpcError::InvalidVersion { low: 1, high: 1 }
        );

        // Verify tasks are running by checking they haven't completed yet
        assert!(!client_write_task.is_finished());
        assert!(!client_read_task.is_finished());
        assert!(!server_write_task.is_finished());
        assert!(!server_read_task.is_finished());

        // Clean shutdown - abort the tasks since they run indefinitely
        client_write_task.abort();
        client_read_task.abort();
        server_write_task.abort();
        server_read_task.abort();

        // Verify the tasks were aborted (which means they were running)
        assert!(client_write_task.await.unwrap_err().is_cancelled());
        assert!(client_read_task.await.unwrap_err().is_cancelled());
        assert!(server_write_task.await.unwrap_err().is_cancelled());
        assert!(server_read_task.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn test_rpc_server() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let mut service_map = ServiceMap::new();
        service_map.add_service(Box::new(EchoCallbackSerivce {}));

        let server = RpcServer::new(service_map, listener);

        tokio::spawn(async move {
            server.run().await.unwrap();
        });

        let (client_write_tx, client_write_rx) = mpsc::unbounded_channel();
        let (client_read, client_write) = connect(addr).await.unwrap();

        let client_handle = Arc::new(ClientHandle::new(client_write_tx));

        let client_write_task = tokio::spawn(async move {
            launch_write_task(client_write_rx, client_write)
                .await
                .unwrap();
        });
        let client_read_task = tokio::spawn({
            let client_handle = client_handle.clone();
            async move {
                launch_read_task(Arc::new(ServiceMap::new()), client_handle, client_read)
                    .await
                    .unwrap();
            }
        });

        let response = client_handle
            .as_ref()
            .send_call(ProgVers::new(1, 1), 1, b"hello")
            .await
            .unwrap();
        assert_eq!(response, b"hello");
        assert_eq!(
            client_handle
                .as_ref()
                .client_state
                .read()
                .await
                .pending_requests
                .len(),
            0
        );

        assert!(!client_write_task.is_finished());
        assert!(!client_read_task.is_finished());
        // Clean shutdown - abort the tasks since they run indefinitely
        client_write_task.abort();
        client_read_task.abort();
        // Verify the tasks were aborted (which means they were running)
        assert!(client_write_task.await.unwrap_err().is_cancelled());
        assert!(client_read_task.await.unwrap_err().is_cancelled());
    }
}
