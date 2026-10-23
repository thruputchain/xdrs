//! thruput_rpc_driver benchmark suite.
//!
//! Measures the single automatic dispatch/write path across latency,
//! throughput, bidirectional RPC, and handler fanout workloads.
//!
//! Run: `cargo bench -p thruput-rpc-driver --bench rpc_bench`.
//!
//! # Flags (after `--`)
//!
//! - `--scenario <name>` — run only scenarios whose name contains `<name>`
//! - `--quick`           — reduced iterations for fast smoke runs
//! - `--json`            — write detailed JSON to `target/bench/`
//! - `--gh-bench`        — emit p99 latency JSON to stdout (for github-action-benchmark)
//! - `--tracing <level>` — install a tracing subscriber at the given level (info,
//!   debug) to measure instrumentation overhead (writes to sink)

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::Serialize;
use thruput_rpc_driver::{
    CallBuf, ConnectionConfig, DynamicPool, FixedSerialPool, ProgVers, RpcError, RpcSender,
    RpcServer, connect_client_with_config,
    handler::{RpcHandler, ServiceMap},
};

/// A call carrying `payload`, in a buffer recycled from `sender`.
fn call_with<S: RpcSender + ?Sized>(sender: &S, payload: &[u8]) -> CallBuf {
    let mut call = sender.call_buf();
    call.extend_from_slice(payload);
    call
}
use tokio::net::TcpListener;

// ---------------------------------------------------------------------------
// Result types
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
    dispatch_policy: String,
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
                let peer = thruput_rpc_driver::peer();
                let mut call = peer.call_buf();
                call.extend_from_slice(data);
                let response = peer.send_call(ECHO_PROG, 1, call).await?;
                Ok(response.to_vec())
            }
            _ => Err(RpcError::InvalidProcedure),
        }
    }
    fn prog_vers(&self) -> ProgVers {
        CALLBACK_PROG
    }
}

/// Fan-out handler: calls back to the peer N times in parallel over the same
/// connection, waits for every reply, then replies to the original call.
/// Models a coordinator that must hear from several workers before answering.
struct FanoutService {
    fanout: usize,
}

#[async_trait]
impl RpcHandler for FanoutService {
    async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
        match proc {
            0 => Ok(vec![]),
            1 => {
                let peer = thruput_rpc_driver::peer();
                let mut handles = Vec::with_capacity(self.fanout);
                for _ in 0..self.fanout {
                    let peer = peer.clone();
                    let mut call = peer.call_buf();
                    call.extend_from_slice(data);
                    handles.push(tokio::spawn(async move {
                        peer.send_call(ECHO_PROG, 1, call).await
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

/// Minimal accept-and-return handler that always completes on its first poll.
/// This isolates dispatch overhead; it deliberately excludes the worker-queue
/// submission and application work a real handler performs.
struct ReadySubmitService;

#[async_trait]
impl RpcHandler for ReadySubmitService {
    async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
        match proc {
            // Null proc: pure transport and dispatch overhead.
            0 => Ok(vec![]),
            // Accept-and-return: no await anywhere on this path.
            1 => Ok(data.to_vec()),
            _ => Err(RpcError::InvalidProcedure),
        }
    }
    fn prog_vers(&self) -> ProgVers {
        ECHO_PROG
    }
}

/// Simulates handler contention: all handlers acquire a shared lock before
/// doing work, then release.  Models serialized critical sections within
/// concurrent dispatch.
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

// A separate server runtime is a benchmark topology, not a driver mode.
static SERVER_RUNTIME: std::sync::OnceLock<tokio::runtime::Handle> = std::sync::OnceLock::new();

async fn start_server(service: Box<dyn RpcHandler>) -> SocketAddr {
    start_server_multi(vec![service]).await
}

async fn start_server_multi(services_list: Vec<Box<dyn RpcHandler>>) -> SocketAddr {
    let mut services = ServiceMap::new();
    for svc in services_list {
        services.add_service(svc);
    }
    start_server_with_config(ConnectionConfig::new(None, services))
        .await
        .0
}

async fn start_server_with_config(
    config: ConnectionConfig,
) -> (
    SocketAddr,
    tokio::task::JoinHandle<Result<(), std::io::Error>>,
) {
    let start = async move {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = RpcServer::new_with_config(listener, config).start().await;
        (addr, server)
    };
    match SERVER_RUNTIME.get() {
        Some(runtime) => runtime.spawn(start).await.unwrap(),
        None => start.await,
    }
}

async fn run_closed_loop(
    sender: Arc<dyn RpcSender>,
    progvers: ProgVers,
    proc: u32,
    payload: &[u8],
    concurrency: usize,
    iters: usize,
) -> (Vec<Duration>, Duration) {
    assert!(concurrency > 0, "concurrency must be at least 1");
    let payload: Arc<[u8]> = Arc::from(payload);
    let ready = Arc::new(tokio::sync::Barrier::new(concurrency + 1));
    let start = Arc::new(tokio::sync::Barrier::new(concurrency + 1));
    let mut handles = Vec::with_capacity(concurrency);

    // Use a fixed set of closed-loop workers. Task creation happens before the
    // barrier and is excluded from the timed section.
    for worker in 0..concurrency {
        let sender = sender.clone();
        let payload = payload.clone();
        let ready = ready.clone();
        let start = start.clone();
        let calls = iters / concurrency + usize::from(worker < iters % concurrency);
        handles.push(tokio::spawn(async move {
            let mut samples = Vec::with_capacity(calls);
            ready.wait().await;
            start.wait().await;
            for _ in 0..calls {
                let t0 = Instant::now();
                sender
                    .send_call(progvers, proc, call_with(&*sender, &payload))
                    .await
                    .unwrap();
                samples.push(t0.elapsed());
            }
            (samples, Instant::now())
        }));
    }

    // Do not start timing until every worker has reached the ready barrier.
    ready.wait().await;
    let wall_start = Instant::now();
    start.wait().await;
    let mut samples = Vec::with_capacity(iters);
    let mut wall_end = wall_start;
    for h in handles {
        let (worker_samples, finished) = h.await.unwrap();
        samples.extend(worker_samples);
        wall_end = wall_end.max(finished);
    }
    let wall = wall_end.duration_since(wall_start);
    assert_eq!(samples.len(), iters);
    (samples, wall)
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
            sender
                .send_call(progvers, proc, call_with(&*sender, &payload))
                .await
                .unwrap();
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

fn print_scenario(r: &ScenarioResult, use_stderr: bool) {
    macro_rules! out {
        ($($arg:tt)*) => {
            if use_stderr { eprint!($($arg)*); } else { print!($($arg)*); }
        };
    }
    macro_rules! outln {
        () => { if use_stderr { eprintln!(); } else { println!(); } };
        ($($arg:tt)*) => {
            if use_stderr { eprintln!($($arg)*); } else { println!($($arg)*); }
        };
    }
    if r.timed_out {
        outln!("{}  TIMEOUT", r.name);
        outln!();
        return;
    }
    let lat = r.latency_us.as_ref().unwrap();
    out!("{}  samples={}", r.name, r.samples);
    if let Some(wall) = r.wall_time_ms {
        out!("  wall={:.0}ms", wall);
    }
    outln!();
    outln!(
        "  min  {:>8.1} µs   p50  {:>8.1} µs   p95  {:>8.1} µs   p99  {:>8.1} µs",
        lat.min,
        lat.p50,
        lat.p95,
        lat.p99
    );
    out!("  max  {:>8.1} µs   mean {:>8.1} µs", lat.max, lat.mean);
    if let Some(tp) = r.throughput_calls_per_sec {
        out!("   throughput  {:.0} calls/s", tp);
    }
    outln!("\n");
}

fn write_json(results: &BenchResult) {
    let dir = std::path::Path::new("target/bench");
    std::fs::create_dir_all(dir).unwrap();

    let variant = "automatic";
    let filename = format!("bench-{}-{}.json", results.metadata.git_sha, variant);
    let path = std::env::var_os("RPC_BENCH_OUTPUT")
        .map_or_else(|| dir.join(&filename), std::path::PathBuf::from);
    let json = serde_json::to_string_pretty(results).unwrap();
    std::fs::write(&path, &json).unwrap();
    eprintln!("Results written to {}", path.display());
}

fn to_gh_bench_json(results: &[ScenarioResult]) -> String {
    let mut entries = Vec::new();
    for result in results
        .iter()
        .filter(|r| !r.timed_out)
        .filter(|r| r.params.get("measurement").and_then(|v| v.as_str()) != Some("run"))
    {
        if let Some(latency) = &result.latency_us {
            entries.push(serde_json::json!({
                "name": result.name,
                "value": latency.p99,
                "unit": "µs"
            }));
        }
    }
    serde_json::to_string_pretty(&entries).unwrap()
}

fn build_metadata() -> Metadata {
    let git_sha = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map_or_else(|| "unknown".to_string(), |s| s.trim().to_string());

    let hostname = std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map_or_else(|| "unknown".to_string(), |s| s.trim().to_string());

    let feature_flags = vec![];

    Metadata {
        git_sha,
        timestamp: unix_timestamp(),
        feature_flags,
        dispatch_policy: "automatic".into(),
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
    gh_bench: bool,
    tracing_level: Option<tracing::Level>,
}

fn parse_args() -> CliArgs {
    let args: Vec<String> = std::env::args().collect();
    let mut filter = None;
    let mut quick = false;
    let mut json = false;
    let mut gh_bench = false;
    let mut tracing_level = None;
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
            "--gh-bench" => {
                gh_bench = true;
            }
            "--tracing" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("--tracing requires a level (info, debug)");
                    std::process::exit(1);
                }
                tracing_level = Some(match args[i].as_str() {
                    "info" => tracing::Level::INFO,
                    "debug" => tracing::Level::DEBUG,
                    other => {
                        eprintln!("unknown tracing level: {other} (use info or debug)");
                        std::process::exit(1);
                    }
                });
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
        gh_bench,
        tracing_level,
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
            client
                .send_call(ECHO_PROG, 1, call_with(&*client, payload))
                .await
                .unwrap();
        }

        let wall_start = Instant::now();
        let mut samples = Vec::with_capacity(iters);
        for _ in 0..iters {
            let t0 = Instant::now();
            client
                .send_call(ECHO_PROG, 1, call_with(&*client, payload))
                .await
                .unwrap();
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
            client
                .send_call(ECHO_PROG, 1, call_with(&*client, payload))
                .await
                .unwrap();
        }

        let (mut samples, wall) = run_concurrent(
            client.handle().clone() as Arc<dyn RpcSender>,
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
        client.send_call(CALLBACK_PROG, 1, call_with(&*client, payload)),
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
        client
            .send_call(CALLBACK_PROG, 1, call_with(&*client, payload))
            .await
            .unwrap();
    }

    let wall_start = Instant::now();
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t0 = Instant::now();
        client
            .send_call(CALLBACK_PROG, 1, call_with(&*client, payload))
            .await
            .unwrap();
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
/// reader/writer task pairs and the fixed pool's FIFO worker dispatch. Production
/// results may differ when handlers do real work, network latency is non-trivial,
/// or connection-level fault isolation matters. This benchmark does not model
/// those conditions.
async fn scenario_pool_vs_single(quick: bool) -> Vec<ScenarioResult> {
    let delay = Duration::from_millis(1);
    let addr = start_server(Box::new(DelayEchoService { delay })).await;

    let payload = &[0u8; 64];
    let concurrency = 50;
    let (warmup, iters) = if quick { (10, 100) } else { (50, 500) };

    let mut results = Vec::new();

    // Path A: single concurrent connection
    {
        let client = connect_client_with_config(&addr, ConnectionConfig::default())
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        for _ in 0..warmup {
            client
                .send_call(ECHO_PROG, 1, call_with(&*client, payload))
                .await
                .unwrap();
        }

        let (mut samples, wall) = run_concurrent(
            client.handle().clone() as Arc<dyn RpcSender>,
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
            pool.send_call(ECHO_PROG, 1, call_with(&*pool, payload))
                .await
                .unwrap();
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

async fn scenario_dynamic_pool(quick: bool) -> Vec<ScenarioResult> {
    let delay = Duration::from_millis(1);
    let addr = start_server(Box::new(DelayEchoService { delay })).await;

    let pool = Arc::new(DynamicPool::new(addr, 10, Duration::from_secs(30), None));
    tokio::time::sleep(Duration::from_millis(50)).await;

    let payload = &[0u8; 64];
    let concurrency = 50;
    let (warmup, iters) = if quick { (10, 50) } else { (50, 500) };

    for _ in 0..warmup {
        pool.send_call(ECHO_PROG, 1, call_with(&*pool, payload))
            .await
            .unwrap();
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

    let throughput = iters as f64 / wall.as_secs_f64();
    let stats = compute_latency_stats(&mut samples);

    let mut params = BTreeMap::new();
    params.insert("payload_bytes".into(), serde_json::json!(64));
    params.insert("concurrency".into(), serde_json::json!(concurrency));
    params.insert("connections".into(), serde_json::json!(10));

    vec![ScenarioResult {
        name: "dynamic_pool/10conn/50conc".to_string(),
        params,
        samples: iters,
        timed_out: false,
        latency_us: Some(stats),
        wall_time_ms: Some(wall.as_secs_f64() * 1000.0),
        throughput_calls_per_sec: Some(throughput),
    }]
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
            client
                .send_call(ECHO_PROG, 1, call_with(&*client, payload))
                .await
                .unwrap();
        }

        let (mut samples, wall) = run_concurrent(
            client.handle().clone() as Arc<dyn RpcSender>,
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

/// Fan-out flow: client calls server, server handler fans out N sub-RPCs back
/// to the client, collects results, then replies.  Requires bidirectional +
/// concurrent dispatch.
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
            client.send_call(CALLBACK_PROG, 1, call_with(&*client, payload)),
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
            client
                .send_call(CALLBACK_PROG, 1, call_with(&*client, payload))
                .await
                .unwrap();
        }

        let wall_start = Instant::now();
        let mut samples = Vec::with_capacity(iters);
        for _ in 0..iters {
            let t0 = Instant::now();
            client
                .send_call(CALLBACK_PROG, 1, call_with(&*client, payload))
                .await
                .unwrap();
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

async fn measure_ready_dispatch(
    concurrency: usize,
    warmup: usize,
    iters: usize,
) -> (Vec<Duration>, Duration) {
    let mut services = ServiceMap::new();
    services.add_service(Box::new(ReadySubmitService));
    let config = ConnectionConfig::new(None, services);

    let (addr, server) = start_server_with_config(config).await;

    let client = connect_client_with_config(&addr, ConnectionConfig::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let sender = client.handle().clone() as Arc<dyn RpcSender>;

    // Warm up with the same closed-loop concurrency as the measured run.
    let _ = run_closed_loop(
        sender.clone(),
        ECHO_PROG,
        1,
        &[0u8; 64],
        concurrency,
        warmup,
    )
    .await;
    let measured = run_closed_loop(sender, ECHO_PROG, 1, &[0u8; 64], concurrency, iters).await;

    client.shutdown().await;
    server.abort();
    let _ = server.await;
    measured
}

/// Ready-handler throughput: the workload the inline fast path exists for.
///
/// Every handler completes on its first poll, so no CALL should spawn a task.
/// Aggregate two repetitions of the single dispatch path.
async fn scenario_ready_dispatch(quick: bool) -> Vec<ScenarioResult> {
    let concurrency_levels: &[usize] = &[1, 10, 50];
    let (warmup, iters) = if quick { (100, 500) } else { (2_000, 100_000) };
    let variants = ["automatic", "automatic"];
    let output_order = ["automatic"];

    let mut results = Vec::new();
    for &concurrency in concurrency_levels {
        let mut measurements: BTreeMap<&str, (Vec<Duration>, Duration, usize)> = BTreeMap::new();
        let mut variant_runs: BTreeMap<&str, usize> = BTreeMap::new();

        for (position, &variant) in variants.iter().enumerate() {
            let (mut samples, wall) = measure_ready_dispatch(concurrency, warmup, iters).await;
            let variant_run = variant_runs.entry(variant).or_default();
            *variant_run += 1;

            let mut params = BTreeMap::new();
            params.insert("payload_bytes".into(), serde_json::json!(64));
            params.insert("concurrency".into(), serde_json::json!(concurrency));
            params.insert("dispatch".into(), serde_json::json!(variant));
            params.insert("measurement".into(), serde_json::json!("run"));
            params.insert("run_position".into(), serde_json::json!(position + 1));
            params.insert("variant_run".into(), serde_json::json!(*variant_run));
            results.push(ScenarioResult {
                name: format!("ready_dispatch/{variant}/{concurrency}/run{variant_run}"),
                params,
                samples: samples.len(),
                timed_out: false,
                latency_us: Some(compute_latency_stats(&mut samples)),
                wall_time_ms: Some(wall.as_secs_f64() * 1000.0),
                throughput_calls_per_sec: Some(samples.len() as f64 / wall.as_secs_f64()),
            });

            let entry = measurements.entry(variant).or_default();
            entry.0.extend(samples);
            entry.1 += wall;
            entry.2 += 1;
        }

        for variant in output_order {
            let (mut samples, wall, runs) = measurements.remove(variant).unwrap();
            let mut params = BTreeMap::new();
            params.insert("payload_bytes".into(), serde_json::json!(64));
            params.insert("concurrency".into(), serde_json::json!(concurrency));
            params.insert("dispatch".into(), serde_json::json!(variant));
            params.insert("measurement".into(), serde_json::json!("aggregate"));
            params.insert("measurement_runs".into(), serde_json::json!(runs));

            results.push(ScenarioResult {
                name: format!("ready_dispatch/{variant}/{concurrency}"),
                params,
                samples: samples.len(),
                timed_out: false,
                latency_us: Some(compute_latency_stats(&mut samples)),
                wall_time_ms: Some(wall.as_secs_f64() * 1000.0),
                throughput_calls_per_sec: Some(samples.len() as f64 / wall.as_secs_f64()),
            });
        }
    }
    results
}

/// Simulates handler contention: concurrent dispatches compete for a shared
/// lock inside the handler.  Models services where some operations serialize
/// through a mutex even under concurrent dispatch.
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
        client
            .send_call(ECHO_PROG, 1, call_with(&*client, payload))
            .await
            .unwrap();
    }

    let (mut samples, wall) = run_concurrent(
        client.handle().clone() as Arc<dyn RpcSender>,
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
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let workers: usize = std::env::var("TOKIO_WORKER_THREADS")
        .unwrap_or_else(|_| "2".into())
        .parse()
        .unwrap();
    let server_workers: usize = std::env::var("RPC_BENCH_SERVER_WORKERS")
        .unwrap_or_else(|_| "0".into())
        .parse()
        .unwrap();
    assert!(
        workers > server_workers,
        "server workers must be less than total workers"
    );
    let build = |count| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(count)
            .enable_all()
            .build()
            .unwrap()
    };
    let server_runtime = (server_workers > 0).then(|| build(server_workers));
    if let Some(runtime) = &server_runtime {
        SERVER_RUNTIME.set(runtime.handle().clone()).unwrap();
    }
    let runtime = build(workers - server_workers);
    runtime.block_on(run());
}

async fn run() {
    let cli = parse_args();

    // Install a tracing subscriber if requested. Writes to sink (no I/O)
    // so we measure pure instrumentation overhead.
    if let Some(level) = cli.tracing_level {
        use tracing_subscriber::fmt::format::FmtSpan;
        tracing_subscriber::fmt()
            .with_max_level(level)
            .with_writer(std::io::sink)
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

    let tracing_label = match cli.tracing_level {
        Some(level) => format!("    tracing: {level} (subscriber → sink)"),
        None => "    tracing: none (no subscriber)".to_string(),
    };

    if cli.gh_bench {
        eprintln!("=== thruput_rpc_driver benchmark (loopback TCP) ===");
        eprintln!("    dispatch: ready inline; suspended handlers spawned");
        eprintln!("{tracing_label}");
        eprintln!();
    } else {
        println!("=== thruput_rpc_driver benchmark (loopback TCP) ===");
        println!("    dispatch: ready inline; suspended handlers spawned");
        println!("{tracing_label}");
        println!();
    }

    let mut all_results = Vec::new();

    if should_run("sequential_latency", &cli.scenario_filter) {
        let results = scenario_sequential_latency(cli.quick).await;
        for r in &results {
            print_scenario(r, cli.gh_bench);
        }
        all_results.extend(results);
    }

    if should_run("ready_dispatch", &cli.scenario_filter) {
        let results = scenario_ready_dispatch(cli.quick).await;
        for r in &results {
            print_scenario(r, cli.gh_bench);
        }
        all_results.extend(results);
    }

    if should_run("concurrent_throughput", &cli.scenario_filter) {
        let results = scenario_concurrent_throughput(cli.quick).await;
        for r in &results {
            print_scenario(r, cli.gh_bench);
        }
        all_results.extend(results);
    }

    if should_run("bidirectional_rpc", &cli.scenario_filter) {
        let results = scenario_bidirectional(cli.quick).await;
        for r in &results {
            print_scenario(r, cli.gh_bench);
        }
        all_results.extend(results);
    }

    if should_run("pool_vs_single", &cli.scenario_filter) {
        let results = scenario_pool_vs_single(cli.quick).await;
        for r in &results {
            print_scenario(r, cli.gh_bench);
        }
        all_results.extend(results);
    }

    if should_run("dynamic_pool", &cli.scenario_filter) {
        let results = scenario_dynamic_pool(cli.quick).await;
        for r in &results {
            print_scenario(r, cli.gh_bench);
        }
        all_results.extend(results);
    }

    if should_run("payload_concurrency", &cli.scenario_filter) {
        let results = scenario_payload_concurrency(cli.quick).await;
        for r in &results {
            print_scenario(r, cli.gh_bench);
        }
        all_results.extend(results);
    }

    if should_run("handler_fanout", &cli.scenario_filter) {
        let results = scenario_handler_fanout(cli.quick).await;
        for r in &results {
            print_scenario(r, cli.gh_bench);
        }
        all_results.extend(results);
    }

    if should_run("handler_contention", &cli.scenario_filter) {
        let results = scenario_handler_contention(cli.quick).await;
        for r in &results {
            print_scenario(r, cli.gh_bench);
        }
        all_results.extend(results);
    }

    assert!(
        all_results
            .iter()
            .all(|row| !row.timed_out && row.samples > 0),
        "benchmark failed or returned no samples"
    );
    if cli.gh_bench {
        println!("{}", to_gh_bench_json(&all_results));
    }

    if cli.json {
        let bench_result = BenchResult {
            metadata: build_metadata(),
            scenarios: all_results,
        };
        write_json(&bench_result);
    }
}
