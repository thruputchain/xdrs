//! Fixed-worker end-to-end RPC measurement against a separate smoke server.
mod profile;
mod runtime;

use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Parser;
use rpc_driver::{ProgVers, RpcSender, connect_client};
use tokio::sync::Barrier;

#[derive(Parser)]
struct Args {
    #[arg(long, required = true, value_delimiter = ',')]
    addr: Vec<std::net::SocketAddr>,
    #[command(flatten)]
    runtime: runtime::RuntimeArgs,
    #[arg(long, default_value_t = 1)]
    connections: usize,
    #[arg(long, default_value_t = 1)]
    concurrency: usize,
    /// Measured calls per worker (warmup excluded).
    #[arg(long, default_value_t = 10_000)]
    iterations: usize,
    #[arg(long, default_value_t = 1000)]
    warmup: usize,
    #[arg(long, default_value_t = 64)]
    payload_bytes: usize,
    /// Decode directly from the reader's slice (prototype builds only).
    #[arg(long)]
    decode_replies: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if args.decode_replies && !cfg!(feature = "reply-decode") {
        return Err("rebuild rpc-smoke with --features reply-decode".into());
    }
    let runtimes = args.runtime.build()?;
    let handles: Vec<_> = runtimes.iter().map(|rt| rt.handle().clone()).collect();
    runtimes[0].block_on(run(args, &handles))
}

async fn run(
    args: Args,
    handles: &[tokio::runtime::Handle],
) -> Result<(), Box<dyn std::error::Error>> {
    if args.connections == 0 || args.concurrency == 0 || args.iterations == 0 {
        return Err("connections, concurrency, and iterations must be positive".into());
    }
    if args.payload_bytes < 5 || args.payload_bytes > 1024 * 1024 {
        return Err("payload-bytes must be between 5 and 1048576".into());
    }
    let mut connections: Vec<Arc<dyn RpcSender>> = Vec::new();
    for connection in 0..args.connections {
        let addr = args.addr[connection % args.addr.len()];
        let client = handles[connection % handles.len()]
            .spawn(async move { connect_client(&addr).await })
            .await??;
        connections.push(Arc::new(client));
    }
    let ready = Arc::new(Barrier::new(args.concurrency + 1));
    let start = Arc::new(Barrier::new(args.concurrency + 1));
    let payload = Arc::new(profile::encode_payload(
        profile::TAG_ECHO,
        0,
        args.payload_bytes,
    ));
    let prog = ProgVers::new(profile::SMOKE_PROG, 1);
    let mut workers = Vec::new();
    for worker in 0..args.concurrency {
        let sender = connections[worker % connections.len()].clone();
        let payload = payload.clone();
        let ready = ready.clone();
        let start = start.clone();
        let iterations = args.iterations;
        let warmup = args.warmup;
        let decode_replies = args.decode_replies;
        let shard = (worker % connections.len()) % handles.len();
        workers.push(handles[shard].spawn(async move {
            for _ in 0..warmup {
                roundtrip(&*sender, prog, &payload, decode_replies).await;
            }
            let mut samples = Vec::with_capacity(iterations);
            ready.wait().await;
            start.wait().await;
            for _ in 0..iterations {
                let before = Instant::now();
                roundtrip(&*sender, prog, &payload, decode_replies).await;
                let elapsed = before.elapsed();
                samples.push(elapsed);
            }
            (samples, Instant::now())
        }));
    }
    let measurement = async {
        ready.wait().await;
        let before = Instant::now();
        start.wait().await;
        let mut finished = before;
        let mut samples = Vec::new();
        for worker in workers {
            let (times, end) = worker.await.unwrap();
            samples.extend(times);
            finished = finished.max(end);
        }
        (samples, finished.duration_since(before))
    };
    // Also bounds a failed worker stranding its peers at the warmup barrier.
    let (mut samples, wall) = tokio::time::timeout(Duration::from_secs(120), measurement).await?;
    samples.sort_unstable();
    let percentile =
        |percent: usize| samples[(samples.len() * percent).div_ceil(100) - 1].as_secs_f64() * 1e6;
    println!(
        "{}",
        serde_json::json!({
            "runtime_shards": handles.len(),
            "connections": args.connections, "concurrency": args.concurrency,
            "iterations_per_worker": args.iterations, "warmup_per_worker": args.warmup,
            "payload_bytes": args.payload_bytes, "completed": samples.len(), "errors": 0,
            "decode_replies": args.decode_replies,
            "wall_seconds": wall.as_secs_f64(),
            "throughput_rps": samples.len() as f64 / wall.as_secs_f64(),
            "latency_us": {"p50": percentile(50), "p95": percentile(95), "p99": percentile(99)}
        })
    );
    Ok(())
}

async fn roundtrip(sender: &dyn RpcSender, prog: ProgVers, payload: &[u8], decoded: bool) {
    let mut call = sender.call_buf();
    call.extend_from_slice(payload);
    #[cfg(feature = "reply-decode")]
    if decoded {
        // SAFETY: payload and callback stay live until completion/cancellation.
        unsafe {
            sender
                .send_call_decoded(prog, 1, call, &mut |bytes| {
                    assert_eq!(bytes, payload);
                    Ok(())
                })
                .await
                .unwrap();
        }
        return;
    }
    let _ = decoded;
    let response = sender.send_call(prog, 1, call).await.unwrap();
    assert_eq!(response.as_ref(), payload);
}
