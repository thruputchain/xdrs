//! RPC round-trip latency benchmark.
//!
//! Measures the end-to-end latency of a single `send_call` → echo → reply cycle
//! over a loopback TCP connection.  Both client and server run in the same
//! tokio runtime, so the numbers represent the overhead of the library
//! (serialisation, channel scheduling, TCP stack) without any real network.
//!
//! Run with:
//!   cargo bench --bench rpc_latency
//!
//! Output example:
//!   === rpc_driver latency benchmark (loopback TCP) ===
//!
//!   null payload (0 bytes)  iters=2000
//!     min    14.2 µs
//!     p50    18.6 µs
//!     p95    31.4 µs
//!     p99    52.1 µs
//!     max   213.0 µs
//!     mean   19.8 µs
//!     throughput  50505 calls/s

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use rpc_driver::{
    ConnectionConfig, ProgVers, RpcError, RpcServer,
    client_state::ClientHandle,
    connect_client_with_config,
    server_state::{ServerDispatch, ServiceMap},
};
use tokio::net::TcpListener;

// ---------------------------------------------------------------------------
// Echo service – echoes the raw payload back unchanged.
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Statistics helpers
// ---------------------------------------------------------------------------

/// Return the value at the given percentile of a *sorted* slice.
fn percentile(sorted: &[Duration], pct: f64) -> Duration {
    assert!(!sorted.is_empty());
    let idx = ((sorted.len() as f64 * pct / 100.0) as usize).min(sorted.len() - 1);
    sorted[idx]
}

fn as_us(d: Duration) -> f64 {
    d.as_secs_f64() * 1_000_000.0
}

// ---------------------------------------------------------------------------
// Benchmark runner
// ---------------------------------------------------------------------------

async fn run(label: &str, client: &ClientHandle, warmup: usize, iters: usize, payload: &[u8]) {
    let prog = ProgVers::new(1, 1);

    // Warm-up: let the tokio scheduler, TCP buffers, and branch predictors settle.
    for _ in 0..warmup {
        client.send_call(prog, 1, payload).await.unwrap();
    }

    // Timed iterations – one call at a time (sequential, not pipelined).
    let mut samples: Vec<Duration> = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t0 = Instant::now();
        client.send_call(prog, 1, payload).await.unwrap();
        samples.push(t0.elapsed());
    }

    samples.sort_unstable();

    let total: Duration = samples.iter().sum();
    let mean = total / iters as u32;
    let throughput = iters as f64 / total.as_secs_f64();

    println!("{label}  iters={iters}");
    println!("  min   {:>8.1} µs", as_us(*samples.first().unwrap()));
    println!("  p50   {:>8.1} µs", as_us(percentile(&samples, 50.0)));
    println!("  p95   {:>8.1} µs", as_us(percentile(&samples, 95.0)));
    println!("  p99   {:>8.1} µs", as_us(percentile(&samples, 99.0)));
    println!("  max   {:>8.1} µs", as_us(*samples.last().unwrap()));
    println!("  mean  {:>8.1} µs", as_us(mean));
    println!("  throughput  {:.0} calls/s", throughput);
    println!();
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    // Bind on an OS-assigned port.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // Register the echo service and start the server.
    let mut services = ServiceMap::new();
    services.add_service(Box::new(EchoService));
    RpcServer::new(listener, services).start().await;

    // Connect the client (timestamp logging off – we measure with Instant).
    let client = connect_client_with_config(&addr, ConnectionConfig::default())
        .await
        .unwrap();

    // Let server tasks fully start before measuring.
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    println!("=== rpc_driver latency benchmark (loopback TCP) ===\n");

    run("null payload (0 bytes)", &client, 200, 2000, b"").await;
    run("small payload (64 bytes)", &client, 200, 2000, &[0u8; 64]).await;
    run("medium payload (1 KiB)", &client, 100, 1000, &[0u8; 1024]).await;
    run("large payload (64 KiB)", &client, 50, 500, &[0u8; 65536]).await;
}
