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
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

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
