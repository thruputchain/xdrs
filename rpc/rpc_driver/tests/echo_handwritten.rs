use std::sync::Arc;

use async_trait::async_trait;
use rpc_driver::{ProgVers, ProgramError, RpcError, client_state::ClientHandle};

#[allow(non_camel_case_types)]
#[async_trait]
pub trait TestProgram_TestVersion: Send + Sync {
    async fn null(&self, client: &Arc<ClientHandle>) -> Result<(), ProgramError>;
    async fn takes_int(&self, client: &Arc<ClientHandle>, i: i32) -> Result<i32, ProgramError>;
    async fn takes_two_ints(
        &self,
        client: &Arc<ClientHandle>,
        i1: i32,
        i2: i32,
    ) -> Result<i32, ProgramError>;

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

#[async_trait]
impl rpc_driver::server_state::ServerDispatch for TestProgram_TestVersion_Dispatcher {
    async fn dispatch(
        &self,
        proc: u32,
        data: &[u8],
        client: &Arc<ClientHandle>,
    ) -> Result<Vec<u8>, RpcError> {
        match proc {
            0 => {
                let _args: () = xdrs::txs::decode(data).map_err(|_| RpcError::GarbageArguments)?;
                let res = self
                    .inner
                    .null(client)
                    .await
                    .map_err(|_| RpcError::SystemError)?;
                xdrs::txs::encode(&res).map_err(|_| RpcError::SystemError)
            }
            1 => {
                let args: (i32,) =
                    xdrs::txs::decode(data).map_err(|_| RpcError::GarbageArguments)?;
                let res = self
                    .inner
                    .takes_int(client, args.0)
                    .await
                    .map_err(|_| RpcError::SystemError)?;
                xdrs::txs::encode(&res).map_err(|_| RpcError::SystemError)
            }
            2 => {
                let args: (i32, i32) =
                    xdrs::txs::decode(data).map_err(|_| RpcError::GarbageArguments)?;
                let res = self
                    .inner
                    .takes_two_ints(client, args.0, args.1)
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
    handle: Arc<ClientHandle>,
}

impl TestProgram_TestVersion_Client {
    pub fn new(handle: Arc<ClientHandle>) -> Self {
        Self { handle }
    }

    fn prog_vers() -> ProgVers {
        ProgVers::new(3123123, 1)
    }

    pub async fn null(&self) -> Result<(), RpcError> {
        let data = xdrs::txs::encode(&()).map_err(|_| RpcError::GarbageArguments)?;
        let buf = self.handle.send_call(Self::prog_vers(), 0, &data).await?;
        xdrs::txs::decode(&buf).map_err(|_| RpcError::SystemError)
    }
    pub async fn takes_int(&self, i: i32) -> Result<i32, RpcError> {
        let data = xdrs::txs::encode(&(i,)).map_err(|_| RpcError::GarbageArguments)?;
        let buf = self.handle.send_call(Self::prog_vers(), 1, &data).await?;
        xdrs::txs::decode(&buf).map_err(|_| RpcError::SystemError)
    }
    pub async fn takes_two_ints(&self, i1: i32, i2: i32) -> Result<i32, RpcError> {
        let data = xdrs::txs::encode(&(i1, i2)).map_err(|_| RpcError::GarbageArguments)?;
        let buf = self.handle.send_call(Self::prog_vers(), 2, &data).await?;
        xdrs::txs::decode(&buf).map_err(|_| RpcError::SystemError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rpc_driver::ProgramError;
    use rpc_driver::client_state::ClientHandle;
    use rpc_driver::tasks::{launch_read_task, launch_write_task};
    use rpc_driver::tcp_connection::connect;
    use rpc_driver::{RpcServer, connect_client, server_state::ServiceMap, tcp_connection::accept};
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tokio::sync::mpsc;

    struct EchoCallbackService {}

    #[async_trait]
    impl TestProgram_TestVersion for EchoCallbackService {
        async fn null(&self, _client: &Arc<ClientHandle>) -> Result<(), ProgramError> {
            Ok(())
        }
        async fn takes_int(
            &self,
            _client: &Arc<ClientHandle>,
            i: i32,
        ) -> Result<i32, ProgramError> {
            println!("EchoCallbackService: takes_int: i={}", i);
            Ok(i)
        }
        async fn takes_two_ints(
            &self,
            _client: &Arc<ClientHandle>,
            i1: i32,
            i2: i32,
        ) -> Result<i32, ProgramError> {
            println!("EchoCallbackService: takes_two_ints: i1={}, i2={}", i1, i2);
            Ok(i1 + i2)
        }
    }

    /// Example service that always returns errors, demonstrating ProgramError usage.
    struct FailingService {}

    #[async_trait]
    impl TestProgram_TestVersion for FailingService {
        async fn null(&self, _client: &Arc<ClientHandle>) -> Result<(), ProgramError> {
            // Using ProgramError::msg() for simple string errors
            Err(ProgramError::msg("null always fails"))
        }

        async fn takes_int(
            &self,
            _client: &Arc<ClientHandle>,
            _i: i32,
        ) -> Result<i32, ProgramError> {
            // Using From<std::io::Error> impl - the ? operator works with io::Error
            let _file = std::fs::File::open("/nonexistent/path/that/doesnt/exist")?;
            Ok(0)
        }

        async fn takes_two_ints(
            &self,
            _client: &Arc<ClientHandle>,
            _i1: i32,
            _i2: i32,
        ) -> Result<i32, ProgramError> {
            // Using ProgramError::new() explicitly for other error types
            Err(ProgramError::new(std::io::Error::new(
                std::io::ErrorKind::Other,
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
        let client = TestProgram_TestVersion_Client::new(connect_client(&addr).await.unwrap());

        println!("client created");
        // Give tasks a moment to start up
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        println!("client null");
        let res = client.null().await.unwrap();
        assert_eq!(res, ());
        println!("client null done");

        let res = client.takes_int(1).await.unwrap();
        assert_eq!(res, 1);

        let res = client.takes_two_ints(1, 2).await.unwrap();
        assert_eq!(res, 3);
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

        // Set up write channel
        let (client_write_tx, client_write_rx) = mpsc::unbounded_channel();
        let (client_handle, client_shutdown_rx) = ClientHandle::new(client_write_tx);
        let client_handle = Arc::new(client_handle);
        let (server_write_tx, server_write_rx) = mpsc::unbounded_channel();
        let (server_handle, server_shutdown_rx) = ClientHandle::new(server_write_tx);
        let server_handle: Arc<ClientHandle> = Arc::new(server_handle);

        // Accept incoming connection in background task - we'll use this for the server side
        let listener_handle =
            tokio::spawn(async move { accept(&listener, 0x100_0000).await.unwrap() });

        // Connect from client side
        let (client_read, client_write) = connect(addr, 0x100_0000).await.unwrap();
        let (server_read, server_write) = listener_handle.await.unwrap();

        let client_write_task = tokio::spawn(async move {
            launch_write_task(client_write_rx, client_write, client_shutdown_rx)
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
            launch_write_task(server_write_rx, server_write, server_shutdown_rx)
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

        let client = TestProgram_TestVersion_Client::new(client_handle.clone());
        let res = client.null().await.unwrap();
        assert_eq!(res, ());

        let res = client.takes_int(1).await.unwrap();
        assert_eq!(res, 1);

        let res = client.takes_two_ints(1, 2).await.unwrap();
        assert_eq!(res, 3);

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

        let client = TestProgram_TestVersion_Client::new(connect_client(&addr).await.unwrap());
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
        async fn null(&self, _client: &Arc<ClientHandle>) -> Result<(), ProgramError> {
            Ok(())
        }
        async fn takes_int(&self, client: &Arc<ClientHandle>, i: i32) -> Result<i32, ProgramError> {
            // Use the typed client wrapper to call back to the client's service
            let cb_client = TestProgram_TestVersion_Client::new(client.clone());
            let notify = self.callback_result.clone();
            tokio::spawn(async move {
                let result = cb_client.takes_int(i).await.unwrap();
                assert_eq!(result, i);
                notify.notify_one();
            });
            // Return immediately — the callback runs after the read loop is unblocked
            Ok(i)
        }
        async fn takes_two_ints(
            &self,
            _client: &Arc<ClientHandle>,
            i1: i32,
            i2: i32,
        ) -> Result<i32, ProgramError> {
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
        let client_handle = rpc_driver::connect_client_with_config(&addr, client_config)
            .await
            .unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let client = TestProgram_TestVersion_Client::new(client_handle);

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
}
