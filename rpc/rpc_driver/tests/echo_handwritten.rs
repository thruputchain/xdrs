use std::sync::Arc;

use async_trait::async_trait;
use rpc_driver::{client_state::ClientHandle, rpc_error::RpcError, ProgVers};

#[allow(non_camel_case_types)]
#[async_trait]
pub trait TestProgram_TestVersion: Send + Sync {
    async fn null(&self) -> ();
    async fn takes_int(&self, i: i32) -> i32;
    async fn takes_two_ints(&self, i1: i32, i2: i32) -> i32;
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
    async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
        match proc {
            0 => {
                let _args: () =
                    xdrs::serialize_txs::decode(data).map_err(|_| RpcError::GarbageArguments)?;
                let res = self.inner.null().await;
                xdrs::serialize_txs::encode(&res).map_err(|_| RpcError::SystemError)
            }
            1 => {
                let args: (i32,) =
                    xdrs::serialize_txs::decode(data).map_err(|_| RpcError::GarbageArguments)?;
                let res = self.inner.takes_int(args.0).await;
                xdrs::serialize_txs::encode(&res).map_err(|_| RpcError::SystemError)
            }
            2 => {
                let args: (i32, i32) =
                    xdrs::serialize_txs::decode(data).map_err(|_| RpcError::GarbageArguments)?;
                let res = self.inner.takes_two_ints(args.0, args.1).await;
                xdrs::serialize_txs::encode(&res).map_err(|_| RpcError::SystemError)
            }
            _ => Err(RpcError::InvalidProcedure),
        }
    }

    fn prog_vers(&self) -> ProgVers {
        ProgVers::new(3123123, 1)
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
        let data = xdrs::serialize_txs::encode(&()).map_err(|_| RpcError::GarbageArguments)?;
        let buf = self.handle.send_call(Self::prog_vers(), 0, &data).await?;
        xdrs::serialize_txs::decode(&buf).map_err(|_| RpcError::SystemError)
    }
    pub async fn takes_int(&self, i: i32) -> Result<i32, RpcError> {
        let data = xdrs::serialize_txs::encode(&(i,)).map_err(|_| RpcError::GarbageArguments)?;
        let buf = self.handle.send_call(Self::prog_vers(), 1, &data).await?;
        xdrs::serialize_txs::decode(&buf).map_err(|_| RpcError::SystemError)
    }
    pub async fn takes_two_ints(&self, i1: i32, i2: i32) -> Result<i32, RpcError> {
        let data =
            xdrs::serialize_txs::encode(&(i1, i2)).map_err(|_| RpcError::GarbageArguments)?;
        let buf = self.handle.send_call(Self::prog_vers(), 2, &data).await?;
        xdrs::serialize_txs::decode(&buf).map_err(|_| RpcError::SystemError)
    }
} 

#[cfg(test)]
mod tests {
    use super::*;
    use rpc_driver::client_state::ClientHandle;
    use rpc_driver::tasks::{launch_read_task, launch_write_task};
    use rpc_driver::tcp_connection::connect;
    use rpc_driver::{server_state::ServiceMap, tcp_connection::accept};
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tokio::sync::mpsc;

    struct EchoCallbackSerivce {}

    #[async_trait]
    impl TestProgram_TestVersion for EchoCallbackSerivce {
        async fn null(&self) -> () {
            ()
        }
        async fn takes_int(&self, i: i32) -> i32 {
            println!("EchoCallbackSerivce: takes_int: i={}", i);
            i
        }
        async fn takes_two_ints(&self, i1: i32, i2: i32) -> i32 {
            println!("EchoCallbackSerivce: takes_two_ints: i1={}, i2={}", i1, i2);
            i1 + i2
        }
    }

    #[tokio::test]
    async fn test_echo_service() {
        // Set up TCP listener for self-connection
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Create service map
        let mut service_map = ServiceMap::new();
        service_map.add_service(Box::new(TestProgram_TestVersion_Dispatcher {
            inner: Box::new(EchoCallbackSerivce {}),
        }));

        // Set up write channel
        let (client_write_tx, client_write_rx) = mpsc::unbounded_channel();
        let client_handle = Arc::new(ClientHandle::new(client_write_tx));
        let (server_write_tx, server_write_rx) = mpsc::unbounded_channel();
        let server_handle: Arc<ClientHandle> = Arc::new(ClientHandle::new(server_write_tx));

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
                launch_read_task(ServiceMap::new(), client_handle, client_read)
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
            launch_read_task(service_map, server_handle.clone(), server_read)
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
}
