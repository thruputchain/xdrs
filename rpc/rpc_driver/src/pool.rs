//! Fixed-size connection pool with FIFO request dispatch.
//!
//! Each connection has a dedicated worker task that processes requests
//! sequentially. Incoming requests are distributed across workers via
//! a shared mpsc channel — the next idle worker picks up the next request.

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};

use crate::{ProgVers, connect_client, rpc_error::RpcError};

/// A request dispatched through the pool to a worker.
struct PoolRequest {
    progvers: ProgVers,
    proc_num: u32,
    data: Vec<u8>,
    reply: oneshot::Sender<Result<Vec<u8>, RpcError>>,
}

/// Fixed-size connection pool with FIFO request dispatch.
///
/// Wraps N [`ClientHandle`] connections to a single address. A shared
/// `tokio::sync::mpsc` channel distributes requests to connection workers
/// in FIFO order — one call at a time per connection.
///
/// This matches the Go `ConnectPoolToScheduler` pattern: a fixed number
/// of connections with sequential dispatch per connection.
#[derive(Debug)]
pub struct FixedPool {
    request_tx: mpsc::Sender<PoolRequest>,
    size: usize,
}

// PoolRequest contains a oneshot::Sender which is not Debug, so we need
// a manual impl for FixedPool only (request_tx is Sender<PoolRequest>).
// The Debug derive on FixedPool works because mpsc::Sender<T> implements
// Debug regardless of T.

impl FixedPool {
    /// Connect `pool_size` clients to `addr`, each with a dedicated worker task.
    ///
    /// All connections are established eagerly. If any connection fails, the
    /// entire pool creation fails and already-spawned workers exit
    /// asynchronously when the sender is dropped.
    pub async fn connect(addr: &SocketAddr, pool_size: usize) -> Result<Self, RpcError> {
        if pool_size == 0 {
            return Err(RpcError::SystemError);
        }

        let (tx, rx) = mpsc::channel::<PoolRequest>(pool_size.saturating_mul(16).max(16));
        let rx = Arc::new(tokio::sync::Mutex::new(rx));

        for _ in 0..pool_size {
            let client = connect_client(addr).await.map_err(|e| {
                eprintln!("pool: failed to connect to {addr}: {e}");
                RpcError::SystemError
            })?;
            let worker_rx = rx.clone();
            tokio::spawn(async move {
                loop {
                    // Acquire the shared receiver lock. The next idle worker
                    // wins the lock and takes the next request.
                    let req = {
                        let mut guard = worker_rx.lock().await;
                        guard.recv().await
                    };
                    // Lock is released here before we do the RPC call,
                    // so other workers can pick up requests concurrently.

                    let req = match req {
                        Some(r) => r,
                        None => break, // Channel closed, pool is shutting down.
                    };

                    let result = client
                        .send_call(req.progvers, req.proc_num, &req.data)
                        .await;
                    let _ = req.reply.send(result);
                }
            });
        }

        Ok(Self {
            request_tx: tx,
            size: pool_size,
        })
    }

    /// Send an RPC call through the pool.
    ///
    /// The request is dispatched to the next idle worker via FIFO ordering.
    /// Returns the response data on success, or an error if the call fails
    /// or the pool has been dropped.
    pub async fn send_call(
        &self,
        progvers: ProgVers,
        proc_num: u32,
        data: &[u8],
    ) -> Result<Vec<u8>, RpcError> {
        let (tx, rx) = oneshot::channel();
        let request = PoolRequest {
            progvers,
            proc_num,
            data: data.to_vec(),
            reply: tx,
        };
        self.request_tx
            .send(request)
            .await
            .map_err(|_| RpcError::SystemError)?;
        rx.await.map_err(|_| RpcError::SystemError)?
    }

    /// Returns the number of connections in the pool.
    pub fn size(&self) -> usize {
        self.size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server_state::{ServerDispatch, ServiceMap};
    use crate::{ClientHandle, RpcServer};
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio::net::TcpListener;

    /// Echo service that returns the input data unchanged.
    struct EchoService;

    #[async_trait]
    impl ServerDispatch for EchoService {
        async fn dispatch(
            &self,
            proc: u32,
            data: &[u8],
            _client: &Arc<ClientHandle>,
        ) -> Result<Vec<u8>, RpcError> {
            if proc != 1 {
                return Err(RpcError::InvalidProcedure);
            }
            Ok(data.to_vec())
        }

        fn prog_vers(&self) -> ProgVers {
            ProgVers::new(1, 1)
        }
    }

    /// Slow echo service that sleeps before responding.
    struct SlowEchoService {
        delay: std::time::Duration,
    }

    #[async_trait]
    impl ServerDispatch for SlowEchoService {
        async fn dispatch(
            &self,
            proc: u32,
            data: &[u8],
            _client: &Arc<ClientHandle>,
        ) -> Result<Vec<u8>, RpcError> {
            if proc != 1 {
                return Err(RpcError::InvalidProcedure);
            }
            tokio::time::sleep(self.delay).await;
            Ok(data.to_vec())
        }

        fn prog_vers(&self) -> ProgVers {
            ProgVers::new(1, 1)
        }
    }

    /// Helper: start an RPC server with the given service and return its address.
    async fn start_echo_server() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut services = ServiceMap::new();
        services.add_service(Box::new(EchoService));
        let server = RpcServer::new(listener, services);
        server.start().await;
        addr
    }

    /// Helper: start a slow RPC server with the given delay.
    async fn start_slow_server(delay: std::time::Duration) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut services = ServiceMap::new();
        services.add_service(Box::new(SlowEchoService { delay }));
        let server = RpcServer::new(listener, services);
        server.start().await;
        addr
    }

    #[tokio::test]
    async fn fixed_pool_distributes_calls() {
        let addr = start_echo_server().await;

        let pool = FixedPool::connect(&addr, 3).await.unwrap();
        assert_eq!(pool.size(), 3);

        // Send 9 calls sequentially and verify all succeed with correct data.
        for i in 0..9u32 {
            let data = format!("call-{}", i);
            let result = pool
                .send_call(ProgVers::new(1, 1), 1, data.as_bytes())
                .await
                .unwrap();
            assert_eq!(result, data.as_bytes());
        }
    }

    #[tokio::test]
    async fn fixed_pool_concurrent_calls() {
        let delay = std::time::Duration::from_millis(100);
        let addr = start_slow_server(delay).await;

        let pool = Arc::new(FixedPool::connect(&addr, 3).await.unwrap());

        let start = tokio::time::Instant::now();

        // Send 3 calls concurrently through a pool of 3 connections.
        // Each call takes ~100ms. With 3 workers, total should be ~100ms.
        let mut handles = Vec::new();
        for i in 0..3u32 {
            let pool = pool.clone();
            handles.push(tokio::spawn(async move {
                let data = format!("slow-{}", i);
                pool.send_call(ProgVers::new(1, 1), 1, data.as_bytes())
                    .await
                    .unwrap()
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        let elapsed = start.elapsed();

        // With 3 workers handling 3 calls in parallel, wall time should be
        // close to one delay (~100ms), not three delays (~300ms).
        // Allow generous margin for CI variance.
        assert!(
            elapsed < std::time::Duration::from_millis(250),
            "Expected parallel execution (~100ms), got {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn fixed_pool_error_propagation() {
        let addr = start_echo_server().await;

        let pool = FixedPool::connect(&addr, 2).await.unwrap();

        // Call an invalid procedure — error should propagate through the pool.
        let result = pool.send_call(ProgVers::new(1, 1), 99, b"bad").await;
        assert_eq!(result, Err(RpcError::InvalidProcedure));

        // Pool should still work for valid calls after an error.
        let result = pool.send_call(ProgVers::new(1, 1), 1, b"ok").await.unwrap();
        assert_eq!(result, b"ok");
    }

    #[tokio::test]
    async fn fixed_pool_zero_size_rejected() {
        let addr = start_echo_server().await;
        let result = FixedPool::connect(&addr, 0).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn fixed_pool_single_connection() {
        let addr = start_echo_server().await;

        let pool = FixedPool::connect(&addr, 1).await.unwrap();
        assert_eq!(pool.size(), 1);

        // Even with one connection, sequential calls should work.
        for i in 0..5u32 {
            let data = format!("single-{}", i);
            let result = pool
                .send_call(ProgVers::new(1, 1), 1, data.as_bytes())
                .await
                .unwrap();
            assert_eq!(result, data.as_bytes());
        }
    }

    #[tokio::test]
    async fn fixed_pool_connect_failure() {
        // Try to connect to an address that's not listening.
        let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
        let result = FixedPool::connect(&addr, 3).await;
        assert!(result.is_err());
    }
}
