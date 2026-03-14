mod metrics;
mod profile;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use clap::Parser;
use rpc_driver::{CpuSetPool, DynamicPool, FixedSerialPool, ProgVers, RpcSender};
use tokio::sync::Semaphore;

use metrics::ClientMetrics;
use profile::*;

#[derive(Parser)]
#[command(name = "rpc-smoke-client")]
struct Args {
    /// Server address (host:port)
    #[arg(long)]
    addr: String,

    /// Profile name (coordinator, gateway, executor, db-shard, auth, echo)
    #[arg(long)]
    profile: String,

    /// Override: number of connections
    #[arg(long)]
    connections: Option<usize>,

    /// Override: test duration in seconds
    #[arg(long)]
    duration: Option<u64>,

    /// Max in-flight RPCs across all connections
    #[arg(long, default_value = "100")]
    max_inflight: usize,

    /// Pool type: "fixed" (default), "cpu" (LLC-aware), or "dynamic" (min-pending)
    #[arg(long, default_value = "fixed")]
    pool: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let profiles = builtin_profiles();
    let mut prof = profiles
        .into_iter()
        .find(|p| p.name == args.profile)
        .unwrap_or_else(|| {
            // Default echo profile for scaling sweep
            PairingProfile {
                name: args.profile.clone(),
                connections: 1,
                payload_bytes: 64,
                tag: TAG_ECHO,
                handler_duration_us: 0,
                duration_secs: 5,
            }
        });

    if let Some(c) = args.connections {
        prof.connections = c;
    }
    if let Some(d) = args.duration {
        prof.duration_secs = d;
    }

    let addr: std::net::SocketAddr = args.addr.parse()?;
    let pool: Arc<dyn RpcSender> = match args.pool.as_str() {
        "cpu" => Arc::new(
            CpuSetPool::connect(&addr)
                .await
                .map_err(|e| format!("failed to connect CpuSetPool to {addr}: {e}"))?,
        ),
        "dynamic" => {
            let dp = Arc::new(DynamicPool::new(
                addr,
                prof.connections,
                std::time::Duration::from_secs(300),
            ));
            eprintln!(
                "[client] using DynamicPool (max {} connections)",
                prof.connections
            );
            dp
        }
        _ => Arc::new(
            FixedSerialPool::connect(&addr, prof.connections)
                .await
                .map_err(|e| {
                    format!(
                        "failed to connect {} connections to {addr}: {e}",
                        prof.connections
                    )
                })?,
        ),
    };

    // Arc<[u8]> avoids cloning the payload bytes on every RPC
    let payload: Arc<[u8]> =
        encode_payload(prof.tag, prof.handler_duration_us, prof.payload_bytes).into();
    let progvers = ProgVers::new(SMOKE_PROG, 1);
    let duration = Duration::from_secs(prof.duration_secs);

    let errors = Arc::new(AtomicU64::new(0));
    let sem = Arc::new(Semaphore::new(args.max_inflight));

    let start = Instant::now();
    let deadline = start + duration;

    // Drive RPCs until deadline. Each task returns its latency via JoinSet
    // (no shared mutex needed).
    let mut tasks = tokio::task::JoinSet::new();
    let mut samples = Vec::new();

    while Instant::now() < deadline {
        // Async acquire — suspends instead of busy-spinning when at capacity
        let permit = match sem.clone().acquire_owned().await {
            Ok(p) => p,
            Err(_) => break, // semaphore closed
        };

        if Instant::now() >= deadline {
            break;
        }

        let pool = pool.clone();
        let payload = payload.clone();
        let errors = errors.clone();

        tasks.spawn(async move {
            let _permit = permit;
            let call_start = Instant::now();
            match pool.send_call(progvers, 1, &payload).await {
                Ok(_) => Some(call_start.elapsed()),
                Err(_) => {
                    errors.fetch_add(1, Ordering::Relaxed);
                    None
                }
            }
        });

        // Reap completed tasks and collect latencies to bound memory
        while let Some(res) = tasks.try_join_next() {
            if let Ok(Some(lat)) = res {
                samples.push(lat);
            }
        }
    }

    // Drain remaining in-flight tasks
    let drain = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(res) = tasks.join_next().await {
            if let Ok(Some(lat)) = res {
                samples.push(lat);
            }
        }
    });
    let _ = drain.await;

    let wall_time = start.elapsed();
    let error_count = errors.load(Ordering::Relaxed);

    let metrics = ClientMetrics::from_samples(
        prof.name,
        prof.connections,
        error_count,
        wall_time,
        &mut samples,
    );

    // Output JSON to stdout for harness collection
    println!("{}", serde_json::to_string(&metrics)?);

    Ok(())
}
