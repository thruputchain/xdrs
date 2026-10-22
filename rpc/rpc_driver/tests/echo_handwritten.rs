/// Explicitly invoked diagnostic for the checked-in generated-style dispatcher.
/// A current-thread runtime counts the complete warmed TCP round trip, including
/// both endpoints. Run with --release --ignored --nocapture and this test's name.
#[cfg(test)]
mod allocation_benchmark {
    use super::*;
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    struct CountingAllocator;
    thread_local! {
        static COUNTS: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
    }
    #[global_allocator]
    static ALLOCATOR: CountingAllocator = CountingAllocator;

    // SAFETY: all allocation operations delegate unchanged to System. The
    // thread-local counters contain no heap allocation and never dereference ptr.
    unsafe impl GlobalAlloc for CountingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let _ = COUNTS.try_with(|counts| {
                if let Some((a, r)) = counts.get() {
                    counts.set(Some((a + 1, r)));
                }
            });
            unsafe { System.alloc(layout) }
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let _ = COUNTS.try_with(|counts| {
                if let Some((a, r)) = counts.get() {
                    counts.set(Some((a + 1, r)));
                }
            });
            unsafe { System.alloc_zeroed(layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            let _ = COUNTS.try_with(|counts| {
                if let Some((a, r)) = counts.get() {
                    counts.set(Some((a, r + 1)));
                }
            });
            unsafe { System.realloc(ptr, layout, size) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    struct Echo {
        yielding: bool,
    }
    #[async_trait]
    impl TestProgram_TestVersion for Echo {
        async fn null(&self) -> Result<(), ProgramError> {
            Ok(())
        }
        async fn takes_int(&self, i: i32) -> Result<i32, ProgramError> {
            if self.yielding {
                tokio::task::yield_now().await;
            }
            Ok(i)
        }
        async fn takes_two_ints(&self, a: i32, b: i32) -> Result<i32, ProgramError> {
            Ok(a + b)
        }
    }

    #[test]
    #[ignore = "explicit performance diagnostic; counts warmed TCP allocations"]
    fn benchmark_checked_in_dispatch() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        for yielding in [false, true] {
            runtime.block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let addr = listener.local_addr().unwrap();
                let mut services = rpc_driver::ServiceMap::new();
                services.add_service(Box::new(TestProgram_TestVersion_Dispatcher::new(Box::new(Echo { yielding }))));
                let server = rpc_driver::RpcServer::new(listener, services).start().await;
                let connection = rpc_driver::connect_client(&addr).await.unwrap();
                let client = TestProgram_TestVersion_Client::new(connection.handle().clone());
                for i in 0..2000 { assert_eq!(client.takes_int(i).await.unwrap(), i); }
                let mut trials = Vec::new();
                for _ in 0..7 {
                    COUNTS.with(|counts| counts.set(Some((0, 0))));
                    let start = std::time::Instant::now();
                    for i in 0..10_000 { assert_eq!(client.takes_int(i).await.unwrap(), i); }
                    let elapsed = start.elapsed();
                    let (allocations, reallocations) = COUNTS.with(|counts| counts.replace(None).unwrap());
                    trials.push(serde_json::json!({
                        "ns_per_call": elapsed.as_nanos() as f64 / 10_000.0,
                        "allocations_per_call": allocations as f64 / 10_000.0,
                        "reallocations_per_call": reallocations as f64 / 10_000.0,
                    }));
                }
                println!("{}", serde_json::json!({"yielding": yielding, "calls_per_trial": 10_000, "trials": trials}));
                connection.shutdown().await;
                server.abort();
            });
        }
    }
}

use std::sync::Arc;

use async_trait::async_trait;
use rpc_driver::{CallBuf, ProgVers, ProgramError, RpcError, RpcSender};

#[allow(non_camel_case_types)]
#[async_trait]
pub trait TestProgram_TestVersion: Send + Sync {
    async fn null(&self) -> Result<(), ProgramError>;
    async fn takes_int(&self, i: i32) -> Result<i32, ProgramError>;
    async fn takes_two_ints(&self, i1: i32, i2: i32) -> Result<i32, ProgramError>;

    fn prog_vers(&self) -> ProgVers {
        ProgVers::new(3123123, 1)
    }
}

#[allow(non_camel_case_types)]
pub struct TestProgram_TestVersion_Dispatcher {
    inner: Box<dyn TestProgram_TestVersion>,
}

impl TestProgram_TestVersion_Dispatcher {
    pub fn new(inner: Box<dyn TestProgram_TestVersion>) -> Self {
        Self { inner }
    }
}

// Manual impl: the boxed trait object is not `Debug`.
impl std::fmt::Debug for TestProgram_TestVersion_Dispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestProgram_TestVersion_Dispatcher")
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl rpc_driver::handler::RpcHandler for TestProgram_TestVersion_Dispatcher {
    async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
        match proc {
            0 => {
                let _args: () = xdrs::txs::decode(data).map_err(|_| RpcError::GarbageArguments)?;
                let res = self.inner.null().await.map_err(|_| RpcError::SystemError)?;
                xdrs::txs::encode(&res).map_err(|_| RpcError::SystemError)
            }
            1 => {
                let args: (i32,) =
                    xdrs::txs::decode(data).map_err(|_| RpcError::GarbageArguments)?;
                let res = self
                    .inner
                    .takes_int(args.0)
                    .await
                    .map_err(|_| RpcError::SystemError)?;
                xdrs::txs::encode(&res).map_err(|_| RpcError::SystemError)
            }
            2 => {
                let args: (i32, i32) =
                    xdrs::txs::decode(data).map_err(|_| RpcError::GarbageArguments)?;
                let res = self
                    .inner
                    .takes_two_ints(args.0, args.1)
                    .await
                    .map_err(|_| RpcError::SystemError)?;
                xdrs::txs::encode(&res).map_err(|_| RpcError::SystemError)
            }
            _ => Err(RpcError::InvalidProcedure),
        }
    }

    fn prog_vers(&self) -> ProgVers {
        self.inner.prog_vers()
    }
}

#[allow(non_camel_case_types)]
pub struct TestProgram_TestVersion_Client {
    handle: Arc<dyn RpcSender>,
}

// Manual impl: `Arc<dyn RpcSender>` is not `Debug`.
impl std::fmt::Debug for TestProgram_TestVersion_Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestProgram_TestVersion_Client")
            .finish_non_exhaustive()
    }
}

/// What a generated client emits for its argument tuple: serialize straight
/// into the record buffer with the client's own `xdrs`, since `rpc_api`
/// carries no serializer.
fn encode_args<T: xdrs::WriteXDR>(call: &mut CallBuf, args: &T) -> Result<(), RpcError> {
    call.append_with(|buf| {
        let mut writer = xdrs::txs::buf_writer::BufWriter::from(std::mem::take(buf));
        let result = xdrs::write_xdr(args, &mut writer);
        *buf = writer.into();
        result
    })
    .map_err(|_| RpcError::GarbageArguments)
}

impl TestProgram_TestVersion_Client {
    pub fn new(handle: Arc<dyn RpcSender>) -> Self {
        Self { handle }
    }

    fn prog_vers() -> ProgVers {
        ProgVers::new(3123123, 1)
    }

    pub async fn null(&self) -> Result<(), RpcError> {
        let mut call = self.handle.call_buf();
        encode_args(&mut call, &())?;
        // SAFETY: the callback lives across the await and retains no slice.
        unsafe {
            self.handle
                .send_call_decoded(Self::prog_vers(), 0, call, &mut |bytes| {
                    xdrs::txs::decode(bytes).map_err(|_| RpcError::SystemError)
                })
                .await
        }
    }
    pub async fn takes_int(&self, i: i32) -> Result<i32, RpcError> {
        let mut call = self.handle.call_buf();
        encode_args(&mut call, &(i,))?;
        let mut result = None;
        // SAFETY: callback/result live across the await; decoding retains no slice.
        unsafe {
            self.handle
                .send_call_decoded(Self::prog_vers(), 1, call, &mut |bytes| {
                    result = Some(xdrs::txs::decode(bytes).map_err(|_| RpcError::SystemError)?);
                    Ok(())
                })
                .await?;
        }
        Ok(result.unwrap())
    }
    pub async fn takes_two_ints(&self, i1: i32, i2: i32) -> Result<i32, RpcError> {
        let mut call = self.handle.call_buf();
        encode_args(&mut call, &(i1, i2))?;
        let mut result = None;
        // SAFETY: callback/result live across the await; decoding retains no slice.
        unsafe {
            self.handle
                .send_call_decoded(Self::prog_vers(), 2, call, &mut |bytes| {
                    result = Some(xdrs::txs::decode(bytes).map_err(|_| RpcError::SystemError)?);
                    Ok(())
                })
                .await?;
        }
        Ok(result.unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rpc_driver::ProgramError;
    use rpc_driver::connection::ConnectionHandle;
    use rpc_driver::pool::DynamicPool;
    use rpc_driver::tasks::launch_read_task;
    use rpc_driver::tcp_connection::connect;
    use rpc_driver::{RpcServer, connect_client, handler::ServiceMap, tcp_connection::accept};
    use std::sync::Arc;
    use tokio::net::TcpListener;

    struct EchoCallbackService {}

    #[async_trait]
    impl TestProgram_TestVersion for EchoCallbackService {
        async fn null(&self) -> Result<(), ProgramError> {
            Ok(())
        }
        async fn takes_int(&self, i: i32) -> Result<i32, ProgramError> {
            println!("EchoCallbackService: takes_int: i={i}");
            Ok(i)
        }
        async fn takes_two_ints(&self, i1: i32, i2: i32) -> Result<i32, ProgramError> {
            println!("EchoCallbackService: takes_two_ints: i1={i1}, i2={i2}");
            Ok(i1 + i2)
        }
    }

    /// Example service that always returns errors, demonstrating ProgramError usage.
    struct FailingService {}

    #[async_trait]
    impl TestProgram_TestVersion for FailingService {
        async fn null(&self) -> Result<(), ProgramError> {
            Err(ProgramError::msg("null always fails"))
        }

        async fn takes_int(&self, _i: i32) -> Result<i32, ProgramError> {
            let _file = std::fs::File::open("/nonexistent/path/that/doesnt/exist")?;
            Ok(0)
        }

        async fn takes_two_ints(&self, _i1: i32, _i2: i32) -> Result<i32, ProgramError> {
            Err(ProgramError::new(std::io::Error::other(
                "takes_two_ints always fails",
            )))
        }
    }

    #[tokio::test]
    async fn test_echo_client_server() {
        // Set up TCP listener for self-connection
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Create service map
        let mut service_map = ServiceMap::new();
        service_map.add_service(Box::new(TestProgram_TestVersion_Dispatcher {
            inner: Box::new(EchoCallbackService {}),
        }));

        // server
        let server = RpcServer::new(listener, service_map);
        server.start().await;

        println!("server started");
        // client
        let conn = connect_client(&addr).await.unwrap();
        let client = TestProgram_TestVersion_Client::new(conn.handle().clone());

        println!("client created");
        // Give tasks a moment to start up
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        println!("client null");
        client.null().await.unwrap();
        println!("client null done");

        let res = client.takes_int(1).await.unwrap();
        assert_eq!(res, 1);

        let res = client.takes_two_ints(1, 2).await.unwrap();
        assert_eq!(res, 3);
    }

    #[tokio::test]
    async fn test_echo_client_server_dynamic_pool() {
        // Set up TCP listener for self-connection
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Create service map
        let mut service_map = ServiceMap::new();
        service_map.add_service(Box::new(TestProgram_TestVersion_Dispatcher {
            inner: Box::new(EchoCallbackService {}),
        }));

        // server
        let server = RpcServer::new(listener, service_map);
        server.start().await;

        // client backed by a dynamic pool instead of a single connection
        let pool = Arc::new(DynamicPool::new(
            addr,
            4,
            std::time::Duration::from_secs(60),
            None,
        ));
        let client = TestProgram_TestVersion_Client::new(pool.clone());

        // Give tasks a moment to start up
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        client.null().await.unwrap();

        let res = client.takes_int(1).await.unwrap();
        assert_eq!(res, 1);

        let res = client.takes_two_ints(1, 2).await.unwrap();
        assert_eq!(res, 3);

        // Drive enough concurrent calls to force the pool to open multiple
        // connections, then verify every response is correct.
        let mut handles = Vec::new();
        for i in 0..16i32 {
            let client = TestProgram_TestVersion_Client::new(pool.clone());
            handles.push(tokio::spawn(async move {
                let res = client.takes_int(i).await.unwrap();
                assert_eq!(res, i);
            }));
        }
        for handle in handles {
            handle.await.unwrap();
        }

        // The pool should have lazily opened at least one connection.
        assert!(pool.active_connections().await >= 1);
    }

    #[tokio::test]
    async fn test_echo_service() {
        // Set up TCP listener for self-connection
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Create service map
        let mut service_map = ServiceMap::new();
        service_map.add_service(Box::new(TestProgram_TestVersion_Dispatcher {
            inner: Box::new(EchoCallbackService {}),
        }));

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
        let server_handle: Arc<ConnectionHandle> = Arc::new(ConnectionHandle::new(
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

        let client = TestProgram_TestVersion_Client::new(client_handle.clone());
        client.null().await.unwrap();

        let res = client.takes_int(1).await.unwrap();
        assert_eq!(res, 1);

        let res = client.takes_two_ints(1, 2).await.unwrap();
        assert_eq!(res, 3);

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
    async fn test_failing_service() {
        // Test that a service returning errors produces RpcError::SystemError on the client
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let mut service_map = ServiceMap::new();
        service_map.add_service(Box::new(TestProgram_TestVersion_Dispatcher {
            inner: Box::new(FailingService {}),
        }));

        let server = RpcServer::new(listener, service_map);
        server.start().await;

        let conn = connect_client(&addr).await.unwrap();
        let client = TestProgram_TestVersion_Client::new(conn.handle().clone());
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // All methods should return SystemError because the service returns ProgramError
        let res = client.null().await;
        assert_eq!(res, Err(RpcError::SystemError));

        let res = client.takes_int(42).await;
        assert_eq!(res, Err(RpcError::SystemError));

        let res = client.takes_two_ints(1, 2).await;
        assert_eq!(res, Err(RpcError::SystemError));
    }

    // -- Bidirectional RPC test --

    // Server-side service that fires a callback to the client on takes_int,
    // using the typed TestProgram_TestVersion_Client wrapper.
    // The callback is spawned (fire-and-forget) since the read loop is
    // synchronous — the handler must return before the reply can be processed.
    struct CallbackEchoService {
        callback_result: Arc<tokio::sync::Notify>,
    }

    #[async_trait]
    impl TestProgram_TestVersion for CallbackEchoService {
        async fn null(&self) -> Result<(), ProgramError> {
            Ok(())
        }
        async fn takes_int(&self, i: i32) -> Result<i32, ProgramError> {
            // Use rpc_driver::peer() to get the per-connection sender
            let cb_client = TestProgram_TestVersion_Client::new(rpc_driver::peer());
            let notify = self.callback_result.clone();
            tokio::spawn(async move {
                let result = cb_client.takes_int(i).await.unwrap();
                assert_eq!(result, i);
                notify.notify_one();
            });
            // Returning lets the serial read loop process the callback reply.
            Ok(i)
        }
        async fn takes_two_ints(&self, i1: i32, i2: i32) -> Result<i32, ProgramError> {
            Ok(i1 + i2)
        }
    }

    #[tokio::test]
    async fn test_bidirectional_rpc() {
        // Server registers TestProgram service backed by CallbackEchoService
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let callback_received = Arc::new(tokio::sync::Notify::new());

        let mut server_services = ServiceMap::new();
        server_services.add_service(Box::new(TestProgram_TestVersion_Dispatcher::new(Box::new(
            CallbackEchoService {
                callback_result: callback_received.clone(),
            },
        ))));

        let server = RpcServer::new(listener, server_services);
        server.start().await;

        // Client connects with its own ServiceMap containing a TestProgram dispatcher
        let mut client_services = ServiceMap::new();
        client_services.add_service(Box::new(TestProgram_TestVersion_Dispatcher::new(Box::new(
            EchoCallbackService {},
        ))));

        let client_config = rpc_driver::ConnectionConfig::new(None, client_services);
        let conn = rpc_driver::connect_client_with_config(&addr, client_config)
            .await
            .unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let client = TestProgram_TestVersion_Client::new(conn.handle().clone());

        // Client calls server; server's handler fires a callback to the client's service
        let res = client.takes_int(42).await.unwrap();
        assert_eq!(res, 42);

        // Wait for the server's spawned callback to complete
        tokio::time::timeout(
            tokio::time::Duration::from_secs(5),
            callback_received.notified(),
        )
        .await
        .expect("callback was not received within timeout");
    }

    // -- Mock-sender tests (no TCP needed) --

    #[tokio::test]
    async fn test_service_without_sender() {
        let svc = EchoCallbackService {};

        svc.null().await.unwrap();
        assert_eq!(svc.takes_int(42).await.unwrap(), 42);
        assert_eq!(svc.takes_two_ints(10, 20).await.unwrap(), 30);
    }

    #[tokio::test]
    async fn test_typed_client_with_mock_sender() {
        /// Mock that returns a fixed XDR-encoded i32 response.
        struct FixedResponseSender {
            response: Vec<u8>,
        }

        #[async_trait]
        impl RpcSender for FixedResponseSender {
            fn call_buf(&self) -> CallBuf {
                CallBuf::new()
            }

            async fn send_call(
                &self,
                _: ProgVers,
                _: u32,
                _: CallBuf,
            ) -> Result<rpc_driver::RpcResponse, RpcError> {
                Ok(rpc_driver::RpcResponse::from_payload(self.response.clone()))
            }
        }

        let sender = Arc::new(FixedResponseSender {
            response: xdrs::txs::encode(&99i32).unwrap(),
        }) as Arc<dyn RpcSender>;

        let client = TestProgram_TestVersion_Client::new(sender);

        // The client encodes args, sends via mock, decodes the fixed response
        assert_eq!(client.takes_int(42).await.unwrap(), 99);
    }

    #[tokio::test]
    async fn test_failing_service_without_sender() {
        let svc = FailingService {};

        assert!(svc.null().await.is_err());
        assert!(svc.takes_int(1).await.is_err());
        assert!(svc.takes_two_ints(1, 2).await.is_err());
    }
}
