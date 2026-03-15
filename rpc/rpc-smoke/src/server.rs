//! Reference example: RPC server with tracing subscriber setup.
//!
//! This server demonstrates the recommended pattern for consuming
//! `rpc_driver`'s tracing instrumentation. Key points:
//!
//! 1. **Add `tracing-subscriber` to your binary crate** (not the library).
//!    `rpc_driver` emits spans/events via `tracing`; you choose the subscriber.
//!
//! 2. **Initialize the subscriber before `RpcServer::start()`** so that
//!    connection spans are captured from the first accept.
//!
//! 3. **Use `FmtSpan::CLOSE`** to see per-span timing (`time.busy` /
//!    `time.idle`). Without it, only error/warn events appear.
//!
//! 4. **Choose your level:**
//!    - `INFO` — connection lifecycle only, < 1% overhead
//!    - `DEBUG` — per-RPC call spans with xid/prog/vers/proc, ~5% overhead
//!
//! Run with: `rpc-smoke-server --port 9876 --tracing debug`

mod metrics;
mod profile;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use clap::Parser;
use rpc_driver::{
    ConnectionConfig, ProgVers, RpcError, RpcServer,
    handler::{RpcHandler, ServiceMap},
};
use tokio::net::TcpListener;

use profile::*;

/// Shared state for the mutex-contention handler profile.
struct SharedState {
    lock: tokio::sync::Mutex<u64>,
}

/// RPC handler that simulates different workload profiles based on the
/// request payload's tag byte.
struct SmokeHandler {
    shared: Arc<SharedState>,
}

#[async_trait]
impl RpcHandler for SmokeHandler {
    async fn dispatch(&self, _proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
        let (tag, duration_us) = decode_payload(data);
        let dur = Duration::from_micros(duration_us as u64);

        match tag {
            TAG_ECHO => {}
            TAG_SLEEP => {
                tokio::time::sleep(dur).await;
            }
            TAG_CPU_SPIN => {
                tokio::task::spawn_blocking(move || {
                    let deadline = std::time::Instant::now() + dur;
                    while std::time::Instant::now() < deadline {
                        std::hint::spin_loop();
                    }
                })
                .await
                .unwrap();
            }
            TAG_MUTEX => {
                let mut guard = self.shared.lock.lock().await;
                tokio::time::sleep(dur).await;
                *guard += 1;
            }
            TAG_SLEEP_MUTEX => {
                let half = dur / 2;
                // Simulate async I/O
                tokio::time::sleep(half).await;
                // Then contend on shared state
                let mut guard = self.shared.lock.lock().await;
                tokio::time::sleep(half).await;
                *guard += 1;
            }
            _ => {
                eprintln!("[smoke-server] unknown tag: {tag:#x}");
                return Err(RpcError::GarbageArguments);
            }
        }

        // Echo the payload back
        Ok(data.to_vec())
    }

    fn prog_vers(&self) -> ProgVers {
        ProgVers::new(SMOKE_PROG, 1)
    }
}

#[derive(Parser)]
#[command(name = "rpc-smoke-server")]
struct Args {
    /// Address to bind to (use 0.0.0.0 for network access)
    #[arg(long, default_value = "127.0.0.1")]
    bind: String,

    /// Port to listen on (0 = OS-assigned)
    #[arg(long, default_value = "0")]
    port: u16,

    /// Max concurrent dispatches per connection
    #[arg(long, default_value = "64")]
    max_concurrent: usize,

    /// Enable tracing subscriber at the given level (info, debug)
    #[arg(long)]
    tracing: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // -- Tracing subscriber setup (reference example) --
    //
    // rpc_driver emits:
    //   - rpc_connection spans (INFO) with peer address and role
    //   - rpc_call spans (DEBUG) with xid, prog, vers, proc
    //   - error/warn events for connection failures, parse errors, etc.
    //
    // FmtSpan::CLOSE logs each span's duration on close, giving you
    // per-RPC time.busy (handler execution) and time.idle (scheduling).
    // Without CLOSE, only error/warn events produce output.
    //
    // For OpenTelemetry, replace this with tracing-opentelemetry —
    // it captures span start/end natively for Jaeger/Tempo export.
    if let Some(ref level) = args.tracing {
        use tracing_subscriber::fmt::format::FmtSpan;
        let level: tracing::Level = level.parse().map_err(|_| {
            format!("unknown tracing level: {level} (use trace, debug, info, warn, error)")
        })?;
        tracing_subscriber::fmt()
            .with_max_level(level)
            .with_writer(std::io::stderr)
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

    let shared = Arc::new(SharedState {
        lock: tokio::sync::Mutex::new(0),
    });

    let handler = SmokeHandler { shared };

    let mut services = ServiceMap::new();
    services.add_service(Box::new(handler));

    let config =
        ConnectionConfig::new(None, services).with_max_concurrent_dispatches(args.max_concurrent);

    let listener = TcpListener::bind(format!("{}:{}", args.bind, args.port)).await?;
    let addr = listener.local_addr()?;

    let server = RpcServer::new_with_config(listener, config);
    let _handle = server.start().await;

    // Signal readiness to harness
    println!("READY {addr}");

    // Run until killed
    tokio::signal::ctrl_c().await?;
    Ok(())
}
