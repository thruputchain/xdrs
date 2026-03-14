//! rpc_driver benchmark suite.
//!
//! Measures serial and concurrent dispatch performance across multiple
//! scenarios. Outputs human-readable stats to stdout.
//!
//! # Usage
//!
//! ```sh
//! cargo bench --bench rpc_bench
//! cargo bench --bench rpc_bench --features concurrent-dispatch
//! ```
//!
//! # Flags (after `--`)
//!
//! - `--scenario <name>` — run only scenarios whose name contains `<name>`
//! - `--quick`           — 10x fewer iterations for fast smoke runs
//! - `--json`            — write detailed JSON to `target/bench/` (for bench-compare)
//! - `--bencher`         — emit Bencher Metric Format JSON to stdout (for CI)

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use rpc_driver::{
    ConnectionConfig, FixedSerialPool, ProgVers, RpcError, RpcSender, RpcServer,
    connect_client_with_config,
    handler::{RpcHandler, ServiceMap},
};
use serde::Serialize;
use tokio::net::TcpListener;

// ---------------------------------------------------------------------------
// Result types (mirrored from bench-compare/src/types.rs — shared via JSON)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct BenchResult {
    metadata: Metadata,
    scenarios: Vec<ScenarioResult>,
}

#[derive(Debug, Serialize)]
struct Metadata {
    git_sha: String,
    timestamp: String,
    feature_flags: Vec<String>,
    hostname: String,
    rpc_driver_version: String,
}

#[derive(Debug, Serialize)]
struct ScenarioResult {
    name: String,
    params: BTreeMap<String, serde_json::Value>,
    samples: usize,
    timed_out: bool,
    latency_us: Option<LatencyStats>,
    wall_time_ms: Option<f64>,
    throughput_calls_per_sec: Option<f64>,
}

#[derive(Debug, Serialize)]
struct LatencyStats {
    min: f64,
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
    mean: f64,
}

// ---------------------------------------------------------------------------
// Statistics helpers
// ---------------------------------------------------------------------------

fn percentile(sorted: &[Duration], pct: f64) -> Duration {
    assert!(!sorted.is_empty());
    let idx = ((sorted.len() as f64 * pct / 100.0).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len() - 1);
    sorted[idx]
}

fn as_us(d: Duration) -> f64 {
    d.as_secs_f64() * 1_000_000.0
}

fn compute_latency_stats(samples: &mut [Duration]) -> LatencyStats {
    samples.sort_unstable();
    let total: Duration = samples.iter().sum();
    let mean = total / samples.len() as u32;
    LatencyStats {
        min: as_us(*samples.first().unwrap()),
        p50: as_us(percentile(samples, 50.0)),
        p95: as_us(percentile(samples, 95.0)),
        p99: as_us(percentile(samples, 99.0)),
        max: as_us(*samples.last().unwrap()),
        mean: as_us(mean),
    }
}

// ---------------------------------------------------------------------------
// Services
// ---------------------------------------------------------------------------

const ECHO_PROG: ProgVers = ProgVers::new(1, 1);

struct EchoService;

#[async_trait]
impl RpcHandler for EchoService {
    async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
        match proc {
            0 => Ok(vec![]),
            1 => Ok(data.to_vec()),
            _ => Err(RpcError::InvalidProcedure),
        }
    }
    fn prog_vers(&self) -> ProgVers {
        ECHO_PROG
    }
}

/// Echo service with a simulated I/O delay.
struct DelayEchoService {
    delay: Duration,
}

#[async_trait]
impl RpcHandler for DelayEchoService {
    async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
        match proc {
            0 => Ok(vec![]),
            1 => {
                tokio::time::sleep(self.delay).await;
                Ok(data.to_vec())
            }
            _ => Err(RpcError::InvalidProcedure),
        }
    }
    fn prog_vers(&self) -> ProgVers {
        ECHO_PROG
    }
}

/// Service that calls back to the peer inline (no spawn).
struct InlineCallbackService;

const CALLBACK_PROG: ProgVers = ProgVers::new(2, 1);

#[async_trait]
impl RpcHandler for InlineCallbackService {
    async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
        match proc {
            0 => Ok(vec![]),
            1 => {
                // Call back to the peer on the same connection, inline.
                let peer = rpc_driver::peer();
                let response = peer.send_call(ECHO_PROG, 1, data).await?;
                Ok(response)
            }
            _ => Err(RpcError::InvalidProcedure),
        }
    }
    fn prog_vers(&self) -> ProgVers {
        CALLBACK_PROG
    }
}

/// Simulates hogd scheduler: handler fans out N sub-RPCs to "executors" via
/// the peer connection, collects results, then returns.  Models the
/// `schedulercoord_nominate` flow where the mempool calls executors in
/// parallel before replying.
struct FanoutService {
    fanout: usize,
}

#[async_trait]
impl RpcHandler for FanoutService {
    async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
        match proc {
            0 => Ok(vec![]),
            1 => {
                let peer = rpc_driver::peer();
                let mut handles = Vec::with_capacity(self.fanout);
                for _ in 0..self.fanout {
                    let peer = peer.clone();
                    let payload = data.to_vec();
                    handles.push(tokio::spawn(async move {
                        peer.send_call(ECHO_PROG, 1, &payload).await
                    }));
                }
                let mut total_len: u32 = 0;
                for h in handles {
                    let resp = h.await.map_err(|_| RpcError::SystemError)??;
                    total_len += resp.len() as u32;
                }
                Ok(total_len.to_be_bytes().to_vec())
            }
            _ => Err(RpcError::InvalidProcedure),
        }
    }
    fn prog_vers(&self) -> ProgVers {
        CALLBACK_PROG
    }
}

/// Simulates handler contention: all handlers acquire a shared lock
/// (like hogd's `commit_lock_`) before doing work, then release.
/// Models serialized critical sections within concurrent dispatch.
struct ContentionService {
    lock: Arc<tokio::sync::Mutex<()>>,
    work_duration: Duration,
}

#[async_trait]
impl RpcHandler for ContentionService {
    async fn dispatch(&self, _proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
        let _guard = self.lock.lock().await;
        tokio::time::sleep(self.work_duration).await;
        Ok(data.to_vec())
    }
    fn prog_vers(&self) -> ProgVers {
        ECHO_PROG
    }
}

// ---------------------------------------------------------------------------
// Setup helpers
// ---------------------------------------------------------------------------

async fn start_server(service: Box<dyn RpcHandler>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut services = ServiceMap::new();
    services.add_service(service);
    RpcServer::new(listener, services).start().await;
    addr
}

async fn start_server_multi(services_list: Vec<Box<dyn RpcHandler>>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut services = ServiceMap::new();
    for svc in services_list {
        services.add_service(svc);
    }
    RpcServer::new(listener, services).start().await;
    addr
}

async fn run_concurrent(
    sender: Arc<dyn RpcSender>,
    progvers: ProgVers,
    proc: u32,
    payload: &[u8],
    concurrency: usize,
    iters: usize,
) -> (Vec<Duration>, Duration) {
    let payload: Arc<[u8]> = Arc::from(payload);
    let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));
    let wall_start = Instant::now();
    let mut handles = Vec::new();
    for _ in 0..iters {
        let sender = sender.clone();
        let payload = payload.clone();
        let sem = semaphore.clone();
        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            let t0 = Instant::now();
            sender.send_call(progvers, proc, &payload).await.unwrap();
            t0.elapsed()
        }));
    }
    let mut samples = Vec::with_capacity(iters);
    for h in handles {
        samples.push(h.await.unwrap());
    }
    let wall = wall_start.elapsed();
    (samples, wall)
}

// ---------------------------------------------------------------------------
// Output helpers
// ---------------------------------------------------------------------------

fn print_scenario(r: &ScenarioResult) {
    if r.timed_out {
        println!("{}  TIMEOUT", r.name);
        println!();
        return;
    }
    let lat = r.latency_us.as_ref().unwrap();
    print!("{}  samples={}", r.name, r.samples);
    if let Some(wall) = r.wall_time_ms {
        print!("  wall={:.0}ms", wall);
    }
    println!();
    println!(
        "  min  {:>8.1} µs   p50  {:>8.1} µs   p95  {:>8.1} µs   p99  {:>8.1} µs",
        lat.min, lat.p50, lat.p95, lat.p99
    );
    print!("  max  {:>8.1} µs   mean {:>8.1} µs", lat.max, lat.mean);
    if let Some(tp) = r.throughput_calls_per_sec {
        print!("   throughput  {:.0} calls/s", tp);
    }
    println!("\n");
}

fn write_json(results: &BenchResult) {
    let dir = std::path::Path::new("target/bench");
    std::fs::create_dir_all(dir).unwrap();

    let variant = if cfg!(feature = "concurrent-dispatch") {
        "concurrent-dispatch"
    } else {
        "serial"
    };
    let filename = format!("bench-{}-{}.json", &results.metadata.git_sha, variant);
    let path = dir.join(&filename);
    let json = serde_json::to_string_pretty(results).unwrap();
    std::fs::write(&path, &json).unwrap();
    println!("Results written to {}", path.display());
}

fn build_metadata() -> Metadata {
    let git_sha = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let hostname = std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let feature_flags = if cfg!(feature = "concurrent-dispatch") {
        vec!["concurrent-dispatch".to_string()]
    } else {
        vec![]
    };

    Metadata {
        git_sha,
        timestamp: unix_timestamp(),
        feature_flags,
        hostname,
        rpc_driver_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

fn unix_timestamp() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    format!("{}", d.as_secs())
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

struct CliArgs {
    scenario_filter: Option<String>,
    quick: bool,
    json: bool,
    bencher: bool,
}

fn parse_args() -> CliArgs {
    let args: Vec<String> = std::env::args().collect();
    let mut filter = None;
    let mut quick = false;
    let mut json = false;
    let mut bencher = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--scenario" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("--scenario requires a value");
                    std::process::exit(1);
                }
                filter = Some(args[i].clone());
            }
            "--quick" => {
                quick = true;
            }
            "--json" => {
                json = true;
            }
            "--bencher" => {
                bencher = true;
            }
            "--bench" => {
                // cargo passes --bench, ignore it
            }
            _ => {}
        }
        i += 1;
    }
    CliArgs {
        scenario_filter: filter,
        quick,
        json,
        bencher,
    }
}

fn should_run(scenario_name: &str, filter: &Option<String>) -> bool {
    match filter {
        Some(f) => scenario_name.contains(f.as_str()),
        None => true,
    }
}

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

async fn scenario_sequential_latency(quick: bool) -> Vec<ScenarioResult> {
    let addr = start_server(Box::new(EchoService)).await;
    let client = connect_client_with_config(&addr, ConnectionConfig::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let payloads: &[(&str, &[u8])] = &[
        ("0B", &[]),
        ("64B", &[0u8; 64]),
        ("1KB", &[0u8; 1024]),
        ("64KB", &[0u8; 65536]),
    ];

    let (warmup, iters) = if quick { (50, 200) } else { (200, 2000) };

    let mut results = Vec::new();
    for (label, payload) in payloads {
        for _ in 0..warmup {
            client.send_call(ECHO_PROG, 1, payload).await.unwrap();
        }

        let wall_start = Instant::now();
        let mut samples = Vec::with_capacity(iters);
        for _ in 0..iters {
            let t0 = Instant::now();
            client.send_call(ECHO_PROG, 1, payload).await.unwrap();
            samples.push(t0.elapsed());
        }
        let wall = wall_start.elapsed();

        let throughput = iters as f64 / wall.as_secs_f64();
        let stats = compute_latency_stats(&mut samples);

        let mut params = BTreeMap::new();
        params.insert(
            "payload_bytes".to_string(),
            serde_json::Value::Number(serde_json::Number::from(payload.len())),
        );
        params.insert(
            "concurrency".to_string(),
            serde_json::Value::Number(serde_json::Number::from(1)),
        );

        results.push(ScenarioResult {
            name: format!("sequential_latency/{label}"),
            params,
            samples: iters,
            timed_out: false,
            latency_us: Some(stats),
            wall_time_ms: Some(wall.as_secs_f64() * 1000.0),
            throughput_calls_per_sec: Some(throughput),
        });
    }
    results
}

async fn scenario_concurrent_throughput(quick: bool) -> Vec<ScenarioResult> {
    let delay = Duration::from_millis(1);
    let addr = start_server(Box::new(DelayEchoService { delay })).await;
    let client = connect_client_with_config(&addr, ConnectionConfig::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let concurrency_levels: &[usize] = &[5, 10, 50];
    let payload = &[0u8; 64];
    let (warmup, iters) = if quick { (10, 100) } else { (50, 1000) };

    let mut results = Vec::new();
    for &concurrency in concurrency_levels {
        for _ in 0..warmup {
            client.send_call(ECHO_PROG, 1, payload).await.unwrap();
        }

        let (mut samples, wall) = run_concurrent(
            client.clone() as Arc<dyn RpcSender>,
            ECHO_PROG,
            1,
            payload,
            concurrency,
            iters,
        )
        .await;

        let throughput = iters as f64 / wall.as_secs_f64();
        let stats = compute_latency_stats(&mut samples);

        let mut params = BTreeMap::new();
        params.insert(
            "payload_bytes".to_string(),
            serde_json::Value::Number(serde_json::Number::from(64)),
        );
        params.insert(
            "concurrency".to_string(),
            serde_json::Value::Number(serde_json::Number::from(concurrency)),
        );

        results.push(ScenarioResult {
            name: format!("concurrent_throughput/{concurrency}/64B"),
            params,
            samples: iters,
            timed_out: false,
            latency_us: Some(stats),
            wall_time_ms: Some(wall.as_secs_f64() * 1000.0),
            throughput_calls_per_sec: Some(throughput),
        });
    }
    results
}

async fn scenario_bidirectional(quick: bool) -> Vec<ScenarioResult> {
    let addr = start_server(Box::new(InlineCallbackService)).await;

    let mut client_services = ServiceMap::new();
    client_services.add_service(Box::new(EchoService));
    let client_config = ConnectionConfig::new(None, client_services);
    let client = connect_client_with_config(&addr, client_config)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let payload = &[0u8; 64];
    let (warmup, iters) = if quick { (5, 50) } else { (20, 200) };
    let timeout_duration = Duration::from_secs(2);

    // Check if bidirectional even works (serial dispatch will deadlock)
    let probe = tokio::time::timeout(
        timeout_duration,
        client.send_call(CALLBACK_PROG, 1, payload),
    )
    .await;

    if probe.is_err() {
        let mut params = BTreeMap::new();
        params.insert(
            "payload_bytes".to_string(),
            serde_json::Value::Number(serde_json::Number::from(64)),
        );
        params.insert(
            "concurrency".to_string(),
            serde_json::Value::Number(serde_json::Number::from(1)),
        );
        return vec![ScenarioResult {
            name: "bidirectional_rpc".to_string(),
            params,
            samples: 0,
            timed_out: true,
            latency_us: None,
            wall_time_ms: None,
            throughput_calls_per_sec: None,
        }];
    }

    // Bidirectional works — fresh connection for clean measurement
    let mut client_services = ServiceMap::new();
    client_services.add_service(Box::new(EchoService));
    let client_config = ConnectionConfig::new(None, client_services);
    let client = connect_client_with_config(&addr, client_config)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    for _ in 0..warmup {
        client.send_call(CALLBACK_PROG, 1, payload).await.unwrap();
    }

    let wall_start = Instant::now();
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t0 = Instant::now();
        client.send_call(CALLBACK_PROG, 1, payload).await.unwrap();
        samples.push(t0.elapsed());
    }
    let wall = wall_start.elapsed();

    let throughput = iters as f64 / wall.as_secs_f64();
    let stats = compute_latency_stats(&mut samples);

    let mut params = BTreeMap::new();
    params.insert(
        "payload_bytes".to_string(),
        serde_json::Value::Number(serde_json::Number::from(64)),
    );
    params.insert(
        "concurrency".to_string(),
        serde_json::Value::Number(serde_json::Number::from(1)),
    );

    vec![ScenarioResult {
        name: "bidirectional_rpc".to_string(),
        params,
        samples: iters,
        timed_out: false,
        latency_us: Some(stats),
        wall_time_ms: Some(wall.as_secs_f64() * 1000.0),
        throughput_calls_per_sec: Some(throughput),
    }]
}

/// Compares a single multiplexed connection against a pool of connections.
///
/// **Caveat:** On loopback with near-zero handler latency, a single concurrent
/// connection consistently outperforms the 10-connection pool (~5x in testing).
/// This is because XID multiplexing over one socket avoids the overhead of 10
/// reader tasks, 10 write mutexes, and FIFO channel dispatch. However, pools
/// are expected to win in production where: handlers do real work (hiding
/// connection overhead), network latency is non-trivial, write mutex contention
/// matters under heavy bidirectional traffic, and fault isolation is required.
/// This benchmark does NOT test those conditions — take the comparison with a
/// grain of salt.
async fn scenario_pool_vs_single(quick: bool) -> Vec<ScenarioResult> {
    let delay = Duration::from_millis(1);
    let addr = start_server(Box::new(DelayEchoService { delay })).await;

    let payload = &[0u8; 64];
    let concurrency = 50;
    let (warmup, iters) = if quick { (10, 100) } else { (50, 500) };

    let mut results = Vec::new();

    // Path A: single concurrent connection (only meaningful with concurrent-dispatch)
    #[cfg(feature = "concurrent-dispatch")]
    {
        let client = connect_client_with_config(&addr, ConnectionConfig::default())
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        for _ in 0..warmup {
            client.send_call(ECHO_PROG, 1, payload).await.unwrap();
        }

        let (mut samples, wall) = run_concurrent(
            client.clone() as Arc<dyn RpcSender>,
            ECHO_PROG,
            1,
            payload,
            concurrency,
            iters,
        )
        .await;

        let mut params = BTreeMap::new();
        params.insert("payload_bytes".into(), serde_json::json!(64));
        params.insert("concurrency".into(), serde_json::json!(concurrency));
        params.insert("connections".into(), serde_json::json!(1));

        results.push(ScenarioResult {
            name: "pool_vs_single/1conn_concurrent".to_string(),
            params,
            samples: iters,
            timed_out: false,
            latency_us: Some(compute_latency_stats(&mut samples)),
            wall_time_ms: Some(wall.as_secs_f64() * 1000.0),
            throughput_calls_per_sec: Some(iters as f64 / wall.as_secs_f64()),
        });
    }

    // Path B: pool of 10 connections (works in both modes)
    {
        let pool = Arc::new(FixedSerialPool::connect(&addr, 10).await.unwrap());
        tokio::time::sleep(Duration::from_millis(50)).await;

        for _ in 0..warmup {
            pool.send_call(ECHO_PROG, 1, payload).await.unwrap();
        }

        let (mut samples, wall) = run_concurrent(
            pool.clone() as Arc<dyn RpcSender>,
            ECHO_PROG,
            1,
            payload,
            concurrency,
            iters,
        )
        .await;

        let mut params = BTreeMap::new();
        params.insert("payload_bytes".into(), serde_json::json!(64));
        params.insert("concurrency".into(), serde_json::json!(concurrency));
        params.insert("connections".into(), serde_json::json!(10));

        results.push(ScenarioResult {
            name: "pool_vs_single/10conn_pool".to_string(),
            params,
            samples: iters,
            timed_out: false,
            latency_us: Some(compute_latency_stats(&mut samples)),
            wall_time_ms: Some(wall.as_secs_f64() * 1000.0),
            throughput_calls_per_sec: Some(iters as f64 / wall.as_secs_f64()),
        });
    }

    results
}

async fn scenario_payload_concurrency(quick: bool) -> Vec<ScenarioResult> {
    let addr = start_server(Box::new(EchoService)).await;
    let client = connect_client_with_config(&addr, ConnectionConfig::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let concurrency = 10;
    let payloads: &[(&str, &[u8])] = &[
        ("64B", &[0u8; 64]),
        ("1KB", &[0u8; 1024]),
        ("64KB", &[0u8; 65536]),
    ];
    let (warmup, iters) = if quick { (10, 100) } else { (50, 500) };

    let mut results = Vec::new();
    for (label, payload) in payloads {
        for _ in 0..warmup {
            client.send_call(ECHO_PROG, 1, payload).await.unwrap();
        }

        let (mut samples, wall) = run_concurrent(
            client.clone() as Arc<dyn RpcSender>,
            ECHO_PROG,
            1,
            payload,
            concurrency,
            iters,
        )
        .await;

        let mut params = BTreeMap::new();
        params.insert("payload_bytes".into(), serde_json::json!(payload.len()));
        params.insert("concurrency".into(), serde_json::json!(concurrency));

        results.push(ScenarioResult {
            name: format!("payload_concurrency/{label}"),
            params,
            samples: iters,
            timed_out: false,
            latency_us: Some(compute_latency_stats(&mut samples)),
            wall_time_ms: Some(wall.as_secs_f64() * 1000.0),
            throughput_calls_per_sec: Some(iters as f64 / wall.as_secs_f64()),
        });
    }
    results
}

// ---------------------------------------------------------------------------
// Production-pattern scenarios
// ---------------------------------------------------------------------------

/// Simulates hogd's nominate flow: client calls server, server handler fans
/// out N sub-RPCs back to the client (like executor calls), collects results,
/// then replies.  Requires bidirectional + concurrent dispatch.
async fn scenario_handler_fanout(quick: bool) -> Vec<ScenarioResult> {
    let fanout_levels: &[usize] = &[2, 5, 10];
    let payload = &[0u8; 64];
    let timeout_duration = Duration::from_secs(5);
    let (warmup, iters) = if quick { (5, 50) } else { (10, 200) };

    let mut results = Vec::new();
    for &fanout in fanout_levels {
        let addr = start_server_multi(vec![
            Box::new(FanoutService { fanout }),
            Box::new(EchoService),
        ])
        .await;

        // Client needs EchoService so server's fanout sub-RPCs have a target
        let mut client_services = ServiceMap::new();
        client_services.add_service(Box::new(EchoService));
        let config = ConnectionConfig::new(None, client_services);
        let client = connect_client_with_config(&addr, config).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Probe — if bidirectional doesn't work, skip
        let probe = tokio::time::timeout(
            timeout_duration,
            client.send_call(CALLBACK_PROG, 1, payload),
        )
        .await;

        if probe.is_err() {
            let mut params = BTreeMap::new();
            params.insert("fanout".into(), serde_json::json!(fanout));
            results.push(ScenarioResult {
                name: format!("handler_fanout/{fanout}"),
                params,
                samples: 0,
                timed_out: true,
                latency_us: None,
                wall_time_ms: None,
                throughput_calls_per_sec: None,
            });
            continue;
        }

        for _ in 0..warmup {
            client.send_call(CALLBACK_PROG, 1, payload).await.unwrap();
        }

        let wall_start = Instant::now();
        let mut samples = Vec::with_capacity(iters);
        for _ in 0..iters {
            let t0 = Instant::now();
            client.send_call(CALLBACK_PROG, 1, payload).await.unwrap();
            samples.push(t0.elapsed());
        }
        let wall = wall_start.elapsed();

        let mut params = BTreeMap::new();
        params.insert("fanout".into(), serde_json::json!(fanout));

        results.push(ScenarioResult {
            name: format!("handler_fanout/{fanout}"),
            params,
            samples: iters,
            timed_out: false,
            latency_us: Some(compute_latency_stats(&mut samples)),
            wall_time_ms: Some(wall.as_secs_f64() * 1000.0),
            throughput_calls_per_sec: Some(iters as f64 / wall.as_secs_f64()),
        });
    }
    results
}

/// Simulates bidirectional load: client sends concurrent requests while the
/// server is also calling back.  Models coordinator sending submit_unvetted
/// while scheduler pushes notify_vetted on the same connection.
async fn scenario_bidirectional_load(quick: bool) -> Vec<ScenarioResult> {
    let addr =
        start_server_multi(vec![Box::new(InlineCallbackService), Box::new(EchoService)]).await;

    let mut client_services = ServiceMap::new();
    client_services.add_service(Box::new(EchoService));
    let config = ConnectionConfig::new(None, client_services);
    let client = connect_client_with_config(&addr, config).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let payload = &[0u8; 64];
    let timeout_duration = Duration::from_secs(2);

    // Probe
    let probe = tokio::time::timeout(
        timeout_duration,
        client.send_call(CALLBACK_PROG, 1, payload),
    )
    .await;

    if probe.is_err() {
        let mut params = BTreeMap::new();
        params.insert("concurrency".into(), serde_json::json!(10));
        return vec![ScenarioResult {
            name: "bidirectional_load".to_string(),
            params,
            samples: 0,
            timed_out: true,
            latency_us: None,
            wall_time_ms: None,
            throughput_calls_per_sec: None,
        }];
    }

    // Fresh connection for measurement
    let mut client_services = ServiceMap::new();
    client_services.add_service(Box::new(EchoService));
    let config = ConnectionConfig::new(None, client_services);
    let client = connect_client_with_config(&addr, config).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let concurrency = 10;
    let (warmup, iters) = if quick { (5, 50) } else { (10, 200) };

    for _ in 0..warmup {
        client.send_call(CALLBACK_PROG, 1, payload).await.unwrap();
    }

    let (mut samples, wall) = run_concurrent(
        client.clone() as Arc<dyn RpcSender>,
        CALLBACK_PROG,
        1,
        payload,
        concurrency,
        iters,
    )
    .await;

    let mut params = BTreeMap::new();
    params.insert("concurrency".into(), serde_json::json!(concurrency));

    vec![ScenarioResult {
        name: "bidirectional_load".to_string(),
        params,
        samples: iters,
        timed_out: false,
        latency_us: Some(compute_latency_stats(&mut samples)),
        wall_time_ms: Some(wall.as_secs_f64() * 1000.0),
        throughput_calls_per_sec: Some(iters as f64 / wall.as_secs_f64()),
    }]
}

/// Simulates handler contention: concurrent dispatches compete for a shared
/// lock inside the handler.  Models hogd's `commit_lock_` pattern where some
/// operations serialize through a mutex even under concurrent dispatch.
/// Shows whether concurrent dispatch still helps when the lock is a bottleneck.
async fn scenario_handler_contention(quick: bool) -> Vec<ScenarioResult> {
    let lock = Arc::new(tokio::sync::Mutex::new(()));
    let work_duration = Duration::from_millis(1);
    let addr = start_server(Box::new(ContentionService {
        lock: lock.clone(),
        work_duration,
    }))
    .await;

    let client = connect_client_with_config(&addr, ConnectionConfig::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let payload = &[0u8; 64];
    let concurrency = 10;
    let (warmup, iters) = if quick { (5, 50) } else { (20, 200) };

    // Warmup
    for _ in 0..warmup {
        client.send_call(ECHO_PROG, 1, payload).await.unwrap();
    }

    let (mut samples, wall) = run_concurrent(
        client.clone() as Arc<dyn RpcSender>,
        ECHO_PROG,
        1,
        payload,
        concurrency,
        iters,
    )
    .await;

    let mut params = BTreeMap::new();
    params.insert("concurrency".into(), serde_json::json!(concurrency));
    params.insert("lock_hold_ms".into(), serde_json::json!(1));

    vec![ScenarioResult {
        name: "handler_contention".to_string(),
        params,
        samples: iters,
        timed_out: false,
        latency_us: Some(compute_latency_stats(&mut samples)),
        wall_time_ms: Some(wall.as_secs_f64() * 1000.0),
        throughput_calls_per_sec: Some(iters as f64 / wall.as_secs_f64()),
    }]
}

// ---------------------------------------------------------------------------
// Bencher Metric Format (BMF) output
// ---------------------------------------------------------------------------

/// Convert results to Bencher Metric Format (BMF) JSON.
///
/// BMF shape: `{ "benchmark_name": { "measure": { "value", "lower_value", "upper_value" } } }`
/// We emit two measures per scenario:
///   - `latency` (µs): value=p50, lower_value=min, upper_value=p99
///   - `throughput` (calls/s): value=throughput (no bounds)
///
/// Timed-out scenarios are omitted (no metrics to report).
fn to_bmf(results: &[ScenarioResult]) -> serde_json::Value {
    let mut bmf = serde_json::Map::new();
    for r in results {
        if r.timed_out {
            continue;
        }
        let mut measures = serde_json::Map::new();
        if let Some(lat) = &r.latency_us {
            measures.insert(
                "latency".to_string(),
                serde_json::json!({
                    "value": lat.p50,
                    "lower_value": lat.min,
                    "upper_value": lat.p99,
                }),
            );
        }
        if let Some(tp) = r.throughput_calls_per_sec {
            measures.insert(
                "throughput".to_string(),
                serde_json::json!({
                    "value": tp,
                }),
            );
        }
        bmf.insert(r.name.clone(), serde_json::Value::Object(measures));
    }
    serde_json::Value::Object(bmf)
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let cli = parse_args();

    if !cli.bencher {
        println!("=== rpc_driver benchmark (loopback TCP) ===");
        if cfg!(feature = "concurrent-dispatch") {
            println!("    dispatch: concurrent (spawn + Semaphore)");
        } else {
            println!("    dispatch: serial (inline await)");
        }
        println!();
    }

    let mut all_results = Vec::new();

    if should_run("sequential_latency", &cli.scenario_filter) {
        let results = scenario_sequential_latency(cli.quick).await;
        if !cli.bencher {
            for r in &results {
                print_scenario(r);
            }
        }
        all_results.extend(results);
    }

    if should_run("concurrent_throughput", &cli.scenario_filter) {
        let results = scenario_concurrent_throughput(cli.quick).await;
        if !cli.bencher {
            for r in &results {
                print_scenario(r);
            }
        }
        all_results.extend(results);
    }

    if should_run("bidirectional_rpc", &cli.scenario_filter) {
        let results = scenario_bidirectional(cli.quick).await;
        if !cli.bencher {
            for r in &results {
                print_scenario(r);
            }
        }
        all_results.extend(results);
    }

    if should_run("pool_vs_single", &cli.scenario_filter) {
        let results = scenario_pool_vs_single(cli.quick).await;
        if !cli.bencher {
            for r in &results {
                print_scenario(r);
            }
        }
        all_results.extend(results);
    }

    if should_run("payload_concurrency", &cli.scenario_filter) {
        let results = scenario_payload_concurrency(cli.quick).await;
        if !cli.bencher {
            for r in &results {
                print_scenario(r);
            }
        }
        all_results.extend(results);
    }

    if should_run("handler_fanout", &cli.scenario_filter) {
        let results = scenario_handler_fanout(cli.quick).await;
        if !cli.bencher {
            for r in &results {
                print_scenario(r);
            }
        }
        all_results.extend(results);
    }

    if should_run("bidirectional_load", &cli.scenario_filter) {
        let results = scenario_bidirectional_load(cli.quick).await;
        if !cli.bencher {
            for r in &results {
                print_scenario(r);
            }
        }
        all_results.extend(results);
    }

    if should_run("handler_contention", &cli.scenario_filter) {
        let results = scenario_handler_contention(cli.quick).await;
        if !cli.bencher {
            for r in &results {
                print_scenario(r);
            }
        }
        all_results.extend(results);
    }

    if cli.bencher {
        // BMF JSON to stdout — bencher reads this via --adapter json
        let bmf = to_bmf(&all_results);
        println!("{}", serde_json::to_string_pretty(&bmf).unwrap());
    } else if cli.json {
        let bench_result = BenchResult {
            metadata: build_metadata(),
            scenarios: all_results,
        };
        write_json(&bench_result);
    }
}
