use rpc_driver::{
    connection::ConnectionHandle, handler::ServiceMap, tasks::launch_read_task,
    tcp_connection::connect,
};
use tokio::net::TcpListener;

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Weak};

    use async_trait::async_trait;
    use rpc_driver::{
        ConnectionConfig, ProgVers, RpcError, RpcSender, RpcServer, connect_client,
        connect_client_with_config, handler::RpcHandler, tcp_connection::accept,
    };
    use tokio::io::AsyncReadExt;

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

    async fn spawn_close_observer() -> (SocketAddr, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let closed = Arc::new(AtomicUsize::new(0));
        let closed_for_task = closed.clone();

        tokio::spawn(async move {
            loop {
                let (mut stream, _) = match listener.accept().await {
                    Ok(pair) => pair,
                    Err(_) => break,
                };
                let closed_for_conn = closed_for_task.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 1];
                    loop {
                        match stream.read(&mut buf).await {
                            Ok(0) => {
                                closed_for_conn.fetch_add(1, Ordering::SeqCst);
                                break;
                            }
                            Ok(_) => {}
                            Err(_) => {
                                closed_for_conn.fetch_add(1, Ordering::SeqCst);
                                break;
                            }
                        }
                    }
                });
            }
        });

        (addr, closed)
    }

    async fn wait_for_closed(counter: &AtomicUsize, expected: usize) {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
        loop {
            if counter.load(Ordering::SeqCst) >= expected {
                return;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for {} closed connections; saw {}",
                expected,
                counter.load(Ordering::SeqCst)
            );
            tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        }
    }

    async fn wait_for_dropped<T>(weak: &Weak<T>) {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
        loop {
            if weak.upgrade().is_none() {
                return;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for Arc to be dropped"
            );
            tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
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
        let listener_handle = tokio::spawn(async move {
            let (r, w, _addr) = accept(&listener, 0x100_0000, 65536).await.unwrap();
            (r, w)
        });

        // Connect from client side
        let (client_read, client_write) = connect(addr, 0x100_0000, 65536).await.unwrap();
        let (server_read, server_write) = listener_handle.await.unwrap();

        let client_handle = Arc::new(ConnectionHandle::new(
            client_write,
            &Default::default(),
            tracing::Span::none(),
        ));
        let server_handle = Arc::new(ConnectionHandle::new(
            server_write,
            &Default::default(),
            tracing::Span::none(),
        ));

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

        let (client_read, client_write) = connect(addr, 0x100_0000, 65536).await.unwrap();

        let client_handle = Arc::new(ConnectionHandle::new(
            client_write,
            &Default::default(),
            tracing::Span::none(),
        ));

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

    /// Poll `is_poisoned()` with a short interval instead of a fixed sleep,
    /// so the test is robust on slow CI runners.
    async fn wait_for_poison(handle: &ConnectionHandle) {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(2);
        loop {
            if handle.is_poisoned() {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for connection to become poisoned"
            );
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn test_fire_and_forget_after_server_drop() {
        // Server accepts connection and immediately drops it.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            drop(stream);
        });

        let client = connect_client(&addr).await.unwrap();

        // Wait for read task to detect EOF and poison the connection.
        wait_for_poison(&client).await;

        // fire-and-forget on a poisoned connection should return SystemError.
        let result = client
            .send_fire_and_forget(ProgVers::new(1, 1), 1, b"hello")
            .await;
        assert_eq!(result.unwrap_err(), RpcError::SystemError);
        assert_eq!(client.num_pending_requests(), 0);
    }

    #[tokio::test]
    async fn test_fire_and_forget_owned_after_server_drop() {
        // Same as above but for the _owned variant.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            drop(stream);
        });

        let client = connect_client(&addr).await.unwrap();

        // Wait for read task to detect EOF and poison the connection.
        wait_for_poison(&client).await;

        let result = client
            .send_fire_and_forget_owned(ProgVers::new(1, 1), 1, b"hello".to_vec())
            .await;
        assert_eq!(result.unwrap_err(), RpcError::SystemError);
        assert_eq!(client.num_pending_requests(), 0);
    }

    #[tokio::test]
    async fn test_fixed_pool_drop_eventually_closes_connections() {
        let (addr, closed) = spawn_close_observer().await;
        let pool = rpc_driver::pool::FixedSerialPool::connect(&addr, 2)
            .await
            .unwrap();
        drop(pool);
        wait_for_closed(&closed, 2).await;
    }

    #[tokio::test]
    async fn test_dynamic_pool_drop_eventually_closes_connections() {
        let (addr, closed) = spawn_close_observer().await;
        let pool = rpc_driver::pool::DynamicPool::new(addr, 2, std::time::Duration::from_secs(60));
        let _ = pool
            .send_fire_and_forget(ProgVers::new(1, 1), 1, b"open")
            .await;
        drop(pool);
        wait_for_closed(&closed, 1).await;
    }

    #[tokio::test]
    async fn test_dynamic_pool_drop_with_evictor_eventually_closes_connections() {
        let (addr, closed) = spawn_close_observer().await;
        let pool = Arc::new(rpc_driver::pool::DynamicPool::new(
            addr,
            2,
            std::time::Duration::from_millis(50),
        ));
        let _ = pool
            .send_fire_and_forget(ProgVers::new(1, 1), 1, b"open")
            .await;
        let weak = Arc::downgrade(&pool);
        // The evictor must not keep the pool alive after the last user-owned Arc is dropped.
        let _evictor = pool.start_evictor();
        drop(pool);
        wait_for_dropped(&weak).await;
        wait_for_closed(&closed, 1).await;
    }

    #[tokio::test]
    async fn test_cpu_pool_drop_eventually_closes_connections() {
        let (addr, closed) = spawn_close_observer().await;
        let pool = rpc_driver::pool::CpuSetPool::connect_with_map(&addr, 2, vec![0, 1])
            .await
            .unwrap();
        drop(pool);
        wait_for_closed(&closed, 2).await;
    }

    #[tokio::test]
    async fn test_pool_drop_churn_does_not_exhaust_file_descriptors() {
        let (addr, closed) = spawn_close_observer().await;
        let mut expected_closed = 0usize;

        for _ in 0..64 {
            let fixed = rpc_driver::pool::FixedSerialPool::connect(&addr, 4)
                .await
                .unwrap();
            drop(fixed);
            expected_closed += 4;

            let dynamic = Arc::new(rpc_driver::pool::DynamicPool::new(
                addr,
                4,
                std::time::Duration::from_secs(60),
            ));
            let mut handles = Vec::new();
            for i in 0..4u32 {
                let dynamic = dynamic.clone();
                handles.push(tokio::spawn(async move {
                    let data = format!("open-{i}");
                    dynamic
                        .send_fire_and_forget(ProgVers::new(1, 1), 1, data.as_bytes())
                        .await
                        .unwrap();
                }));
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            let dynamic_active = dynamic.active_connections().await;
            assert!(
                dynamic_active >= 2,
                "expected multiple dynamic connections, saw {dynamic_active}"
            );
            expected_closed += dynamic_active;

            for handle in handles {
                handle.await.unwrap();
            }
            drop(dynamic);

            let cpu = rpc_driver::pool::CpuSetPool::connect_with_map(&addr, 4, vec![0, 1, 2, 3])
                .await
                .unwrap();
            drop(cpu);
            expected_closed += 4;

            let target = expected_closed;
            wait_for_closed(&closed, target).await;
        }
    }

    #[tokio::test]
    async fn test_pool_evicts_poisoned_connections() {
        use rpc_driver::pool::DynamicPool;
        use std::time::Duration;

        // Server: accept one connection, then drop it to trigger EOF.
        // Keep the listener alive but don't accept more connections so
        // the evictor can't reconnect.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let barrier = Arc::new(tokio::sync::Notify::new());
        let barrier_clone = barrier.clone();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            drop(stream); // close connection -> read task detects EOF -> poison
            barrier_clone.notify_one();
            // Don't accept again; hold the listener so the address stays valid
            // but reconnections will hang (never accepted).
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
            drop(listener);
        });

        // Use a very short idle_ttl so the evictor ticks quickly.
        let pool = Arc::new(DynamicPool::new(addr, 4, Duration::from_millis(50)));
        let evictor = pool.start_evictor();

        // Open the first connection via fire-and-forget.
        let _ = pool
            .send_fire_and_forget(ProgVers::new(1, 1), 1, b"probe")
            .await;
        assert_eq!(pool.active_connections().await, 1);

        // Wait for the server to drop the stream.
        barrier.notified().await;

        // Wait for the evictor to sweep the poisoned connection.
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(2);
        loop {
            if pool.active_connections().await == 0 {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for pool to evict poisoned connection"
            );
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }

        assert_eq!(pool.active_connections().await, 0);
        evictor.abort();
    }

    #[tokio::test]
    async fn test_pool_skips_poisoned_on_lease() {
        use rpc_driver::pool::DynamicPool;
        use std::time::Duration;

        // Manual server with a blocking echo service on connection 1. Connection
        // 1 uses a blocking handler so we can hold it busy to force a second
        // connection. Connection 2 uses a normal echo handler.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let blocker = Arc::new(tokio::sync::Notify::new());
        let (kill_tx, kill_rx) = tokio::sync::oneshot::channel::<()>();

        let blocker_clone = blocker.clone();
        tokio::spawn(async move {
            // Connection 1 — blocking echo handler.
            let (r1, w1, _) = accept(&listener, 0x100_0000, 65536).await.unwrap();
            let h1 = Arc::new(ConnectionHandle::new(
                w1,
                &Default::default(),
                tracing::Span::none(),
            ));
            let mut svc1 = ServiceMap::new();
            svc1.add_service(Box::new(BlockingEchoService {
                notify: blocker_clone,
            }));
            let h1_clone = h1.clone();
            let read_task_1 = tokio::spawn(async move {
                let _ = launch_read_task(Arc::new(svc1), h1_clone, r1).await;
            });

            // Connection 2 — normal echo handler.
            let (r2, w2, _) = accept(&listener, 0x100_0000, 65536).await.unwrap();
            let h2 = Arc::new(ConnectionHandle::new(
                w2,
                &Default::default(),
                tracing::Span::none(),
            ));
            let mut svc2 = ServiceMap::new();
            svc2.add_service(Box::new(EchoCallbackService {}));
            let h2_clone = h2.clone();
            let read_task_2 = tokio::spawn(async move {
                let _ = launch_read_task(Arc::new(svc2), h2_clone, r2).await;
            });

            // Wait for the kill signal, then poison connection 1.
            let _ = kill_rx.await;
            h1.poison().await;
            read_task_1.abort();

            // Keep connection 2 alive.
            let _ = read_task_2.await;
        });

        let pool = Arc::new(DynamicPool::new(addr, 4, Duration::from_secs(60)));

        // Send a call that blocks on connection 1 (the BlockingEchoService waits
        // for blocker.notify_one()).
        let pool_clone = pool.clone();
        let blocked_call = tokio::spawn(async move {
            pool_clone
                .send_call(ProgVers::new(1, 1), 1, b"blocked")
                .await
        });

        // Give time for the blocked call to be dispatched and hold conn1 busy.
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        assert_eq!(pool.active_connections().await, 1);

        // Send a second call. Connection 1 is busy (pending > 0), so the pool
        // opens connection 2.
        let resp = pool
            .send_call(ProgVers::new(1, 1), 1, b"conn2")
            .await
            .unwrap();
        assert_eq!(&*resp, b"conn2");
        assert_eq!(pool.active_connections().await, 2);

        // Unblock connection 1's call.
        blocker.notify_one();
        let blocked_result = blocked_call.await.unwrap().unwrap();
        assert_eq!(&*blocked_result, b"blocked");

        // Now kill connection 1 on the server side.
        kill_tx.send(()).unwrap();

        // Wait for the pool to detect the poisoned connection and verify the
        // next lease returns a healthy connection (connection 2).
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(2);
        loop {
            match pool.send_call(ProgVers::new(1, 1), 1, b"after").await {
                Ok(resp) => {
                    assert_eq!(&*resp, b"after");
                    break;
                }
                Err(_) => {
                    assert!(
                        tokio::time::Instant::now() < deadline,
                        "timed out waiting for pool to skip poisoned connection"
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                }
            }
        }

        // The poisoned connection should have been swept, leaving 1 healthy conn.
        assert_eq!(pool.active_connections().await, 1);
    }

    #[tokio::test]
    async fn test_all_connections_poisoned_reopens() {
        use rpc_driver::pool::DynamicPool;
        use std::time::Duration;

        // Server: accept first connection and drop it to poison, then accept
        // the replacement connection with an echo service running.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let barrier = Arc::new(tokio::sync::Notify::new());
        let barrier_clone = barrier.clone();

        tokio::spawn(async move {
            // Accept first connection and drop it to trigger EOF/poison.
            let (stream, _) = listener.accept().await.unwrap();
            drop(stream);
            barrier_clone.notify_one();

            // Accept the reconnection and run a proper echo handler.
            let (reader, writer, _addr) = accept(&listener, 0x100_0000, 65536).await.unwrap();
            let handle = Arc::new(ConnectionHandle::new(
                writer,
                &Default::default(),
                tracing::Span::none(),
            ));
            let mut svc = ServiceMap::new();
            svc.add_service(Box::new(EchoCallbackService {}));
            let _ = launch_read_task(Arc::new(svc), handle, reader).await;
        });

        let pool = Arc::new(DynamicPool::new(addr, 4, Duration::from_secs(60)));

        // Open the first connection.
        let _ = pool
            .send_fire_and_forget(ProgVers::new(1, 1), 1, b"probe")
            .await;

        // Wait for the server to drop the first connection, then wait for the
        // read task to detect EOF and poison connection 1.
        barrier.notified().await;

        // The retry loop below handles the case where the read task hasn't
        // poisoned the connection yet — send_call will fail, and the next
        // iteration will sweep the poisoned connection and open a fresh one.
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
        loop {
            match pool.send_call(ProgVers::new(1, 1), 1, b"alive").await {
                Ok(resp) => {
                    assert_eq!(&*resp, b"alive");
                    break;
                }
                Err(_) => {
                    assert!(
                        tokio::time::Instant::now() < deadline,
                        "timed out waiting for pool to reopen after all connections poisoned"
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                }
            }
        }

        // The pool should have at least one healthy connection. It may have more
        // than one if a lease opened a connection before the poison was detected
        // (a race inherent to async TCP), but the important thing is that the pool
        // did not panic and successfully recovered.
        assert!(pool.active_connections().await >= 1);
    }

    #[tokio::test]
    async fn test_fire_and_forget_basic() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let dispatch_count = Arc::new(AtomicU32::new(0));
        let notify = Arc::new(tokio::sync::Notify::new());

        struct CountingService {
            count: Arc<AtomicU32>,
            notify: Arc<tokio::sync::Notify>,
        }

        #[async_trait]
        impl RpcHandler for CountingService {
            async fn dispatch(&self, _proc: u32, _data: &[u8]) -> Result<Vec<u8>, RpcError> {
                self.count.fetch_add(1, Ordering::SeqCst);
                self.notify.notify_one();
                Ok(vec![])
            }
            fn prog_vers(&self) -> ProgVers {
                ProgVers::new(1, 1)
            }
        }

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut services = ServiceMap::new();
        services.add_service(Box::new(CountingService {
            count: dispatch_count.clone(),
            notify: notify.clone(),
        }));
        let server = RpcServer::new(listener, services);
        server.start().await;

        let client = connect_client(&addr).await.unwrap();

        // Fire and forget should return immediately.
        client
            .send_fire_and_forget(ProgVers::new(1, 1), 1, b"hello")
            .await
            .unwrap();

        // Wait for the server to process (reply will be silently discarded).
        tokio::time::timeout(std::time::Duration::from_secs(2), notify.notified())
            .await
            .expect("server should have dispatched the call");

        assert_eq!(dispatch_count.load(Ordering::SeqCst), 1);

        // Regular send_call should still work after fire-and-forget.
        let resp = client
            .send_call(ProgVers::new(1, 1), 1, b"echo")
            .await
            .unwrap();
        assert_eq!(&*resp, b""); // CountingService returns empty vec
    }
}
