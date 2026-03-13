use rpc_driver::{
    connection::ConnectionHandle, handler::ServiceMap, tasks::launch_read_task,
    tcp_connection::connect,
};
use tokio::net::TcpListener;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use rpc_driver::{
        ConnectionConfig, ProgVers, RpcError, RpcServer, connect_client,
        connect_client_with_config, handler::RpcHandler, tcp_connection::accept,
    };

    use super::*;
    struct EchoCallbackService {}

    #[async_trait]
    impl RpcHandler for EchoCallbackService {
        async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
            if proc != 1 {
                return Err(RpcError::InvalidProcedure);
            }
            println!(
                "EchoCallbackService: dispatch: proc={}, data={:?}",
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
        service_map.add_service(Box::new(EchoCallbackService {}));

        // Accept incoming connection in background task - we'll use this for the server side
        let listener_handle =
            tokio::spawn(async move { accept(&listener, 0x100_0000).await.unwrap() });

        // Connect from client side
        let (client_read, client_write) = connect(addr, 0x100_0000).await.unwrap();
        let (server_read, server_write) = listener_handle.await.unwrap();

        let client_handle = Arc::new(ConnectionHandle::new(client_write, false));
        let server_handle = Arc::new(ConnectionHandle::new(server_write, false));

        let client_read_task = tokio::spawn({
            let client_handle = client_handle.clone();
            async move {
                launch_read_task(Arc::new(ServiceMap::new()), client_handle, client_read)
                    .await
                    .unwrap();
            }
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
        assert_eq!(client_handle.as_ref().num_pending_requests(), 0);

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
        assert!(!client_read_task.is_finished());
        assert!(!server_read_task.is_finished());

        // Clean shutdown - abort the tasks since they run indefinitely
        client_read_task.abort();
        server_read_task.abort();

        // Verify the tasks were aborted (which means they were running)
        assert!(client_read_task.await.unwrap_err().is_cancelled());
        assert!(server_read_task.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn test_rpc_server() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let mut service_map = ServiceMap::new();
        service_map.add_service(Box::new(EchoCallbackService {}));

        let server = RpcServer::new(listener, service_map);

        server.start().await;

        let (client_read, client_write) = connect(addr, 0x100_0000).await.unwrap();

        let client_handle = Arc::new(ConnectionHandle::new(client_write, false));

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
        assert_eq!(client_handle.as_ref().num_pending_requests(), 0);

        assert!(!client_read_task.is_finished());
        // Clean shutdown - abort the tasks since they run indefinitely
        client_read_task.abort();
        // Verify the tasks were aborted (which means they were running)
        assert!(client_read_task.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn test_server_closes_connection() {
        // Set up TCP listener
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Server task: accept connection and immediately close it by dropping it
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            // Immediately drop the stream to close the connection
            drop(stream);
        });

        // Give server a moment to start
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Client connects using the high-level API
        let client_handle = connect_client(&addr).await.unwrap();

        // Give client tasks a moment to start up
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Try to make a call - should fail because the connection was closed
        let result = client_handle
            .send_call(ProgVers::new(1, 1), 1, b"test")
            .await;

        println!("result: {:?}", result);

        // The call should fail with a SystemError since the connection was closed
        assert!(result.is_err());
        // The error should be SystemError (connection closed)
        assert_eq!(result.unwrap_err(), RpcError::SystemError);
        assert_eq!(client_handle.num_pending_requests(), 0);
    }

    // Service that waits on a condition variable before responding
    struct BlockingEchoService {
        notify: Arc<tokio::sync::Notify>,
    }

    #[async_trait]
    impl RpcHandler for BlockingEchoService {
        async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
            if proc != 1 {
                return Err(RpcError::InvalidProcedure);
            }
            println!(
                "BlockingEchoService: dispatch: proc={}, data={:?}, waiting for notification",
                proc, data
            );
            // Wait for notification before returning
            self.notify.notified().await;
            println!("BlockingEchoService: notified, returning response");
            Ok(data.to_vec())
        }

        fn prog_vers(&self) -> ProgVers {
            ProgVers::new(1, 1)
        }
    }

    #[tokio::test]
    async fn test_client_closes_connection_server_poison() {
        // Set up RpcServer with a blocking service
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let notify = Arc::new(tokio::sync::Notify::new());
        let service = BlockingEchoService {
            notify: notify.clone(),
        };

        let mut service_map = ServiceMap::new();
        service_map.add_service(Box::new(service));

        let server = RpcServer::new(listener, service_map);
        let server_handle = server.clone();

        // Start server in background
        server_handle.start().await;

        // Give server a moment to start
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        {
            // Connect client using high-level API
            let client_handle = connect_client(&addr).await.unwrap();

            // Give client tasks a moment to start up
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

            // Make a call - this will start processing on the server side
            // The server will wait on the notify, giving us time to close the connection
            let call_handle = tokio::spawn({
                let client_handle = client_handle.clone();
                async move {
                    let res = client_handle
                        .send_call(ProgVers::new(1, 1), 1, b"test")
                        .await;
                    println!("res: {:?}", res);
                    assert!(res.is_err());
                }
            });

            // Give the server a moment to start processing the request and enter the wait
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

            client_handle.poison().await;
            call_handle.await.unwrap();
        }

        notify.notify_one();

        // Give the server a moment to detect the closed connection and call poison()
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        // Verify the server is still running and can accept new connections after handling the error
        let new_client_handle = connect_client(&addr).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Make a call on the new connection - it will also block on the notify
        let new_call_handle = tokio::spawn({
            let new_client_handle = new_client_handle.clone();
            async move {
                new_client_handle
                    .send_call(ProgVers::new(1, 1), 1, b"new_test")
                    .await
            }
        });

        // Give it time to start processing
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Notify to let it complete
        notify.notify_one();

        // Wait for it to complete
        let new_result = new_call_handle.await.unwrap();
        assert!(new_result.is_ok());
        assert_eq!(new_result.unwrap(), b"new_test");
    }

    #[tokio::test]
    async fn test_custom_max_message_size() {
        // Set up RpcServer with a custom max message size of 1KB
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let mut service_map = ServiceMap::new();
        service_map.add_service(Box::new(EchoCallbackService {}));

        let server =
            RpcServer::new_with_config(listener, ConnectionConfig::new(Some(1024), service_map));
        let server_handle = server.clone();

        // Start server in background
        server_handle.start().await;

        // Give server a moment to start
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Connect client with matching max message size
        let client_config = ConnectionConfig::new(Some(1024), ServiceMap::new());
        let client_handle = rpc_driver::connect_client_with_config(&addr, client_config)
            .await
            .unwrap();

        // Give client tasks a moment to start up
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Small message should work fine
        let small_data = vec![0u8; 100];
        let result = client_handle
            .send_call(ProgVers::new(1, 1), 1, &small_data)
            .await;
        assert!(result.is_ok());

        // Message larger than 1KB should fail
        let large_data = vec![0u8; 2000];
        let result = client_handle
            .send_call(ProgVers::new(1, 1), 1, &large_data)
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), RpcError::SystemError);
        assert_eq!(client_handle.num_pending_requests(), 0);
    }

    #[tokio::test]
    async fn test_message_too_large() {
        // Set up RpcServer
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let mut service_map = ServiceMap::new();
        service_map.add_service(Box::new(EchoCallbackService {}));

        let server = RpcServer::new(listener, service_map);
        let server_handle = server.clone();

        // Start server in background
        server_handle.start().await;

        // Give server a moment to start
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Connect client using high-level API
        let client_handle = connect_client(&addr).await.unwrap();

        // Give client tasks a moment to start up
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Create a message that exceeds the max message size (0x100_0000 = 16,777,216 bytes)
        // The message size includes both header and data, so we need data that makes
        // the total exceed the limit. The header is typically around 100-200 bytes,
        // so we'll create data that's larger than the max to be safe.
        const MAX_MESSAGE_SIZE: usize = 0x100_0000; // 16,777,216 bytes
        let large_data = vec![0u8; MAX_MESSAGE_SIZE + 1];

        // Try to send a call with data that's too large
        let result = client_handle
            .send_call(ProgVers::new(1, 1), 1, &large_data)
            .await;

        // The call should fail with SystemError because the connection was closed
        // when the server detected the message was too large
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), RpcError::SystemError);

        // Connection should be closed, so future calls should fail
        let result = client_handle
            .send_call(ProgVers::new(1, 1), 1, b"test")
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), RpcError::SystemError);
        assert_eq!(client_handle.num_pending_requests(), 0);
    }

    #[tokio::test]
    async fn test_message_too_large_for_encode() {
        // Set up RpcServer
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let mut service_map = ServiceMap::new();
        service_map.add_service(Box::new(EchoCallbackService {}));

        let server = RpcServer::new(listener, service_map);
        let server_handle = server.clone();

        // Start server in background
        server_handle.start().await;

        // Give server a moment to start
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Connect client using high-level API
        let client_handle = connect_client_with_config(
            &addr,
            ConnectionConfig::new(Some(0x7FFF_FFFF), ServiceMap::new()),
        )
        .await
        .unwrap();

        // Give client tasks a moment to start up
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Create a message that exceeds the single-part message size limit (0x7FFF_FFFF = 2,147,483,647 bytes)
        let large_data = vec![0u8; 0x7FFF_FFFF + 1];

        // Try to send a call with data that's too large
        let result = client_handle
            .send_call(ProgVers::new(1, 1), 1, &large_data)
            .await;

        // The call should fail with SystemError because the call failed to encode
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), RpcError::SystemError);

        // make sure the callback was cleaned up
        assert_eq!(client_handle.num_pending_requests(), 0);
    }
}
