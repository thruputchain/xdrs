mod metrics;
mod profile;

use std::process::Stdio;

use clap::{Parser, Subcommand};
use metrics::{ClientMetrics, DispatchModeSource, RpcCounts};

#[derive(Parser)]
#[command(name = "rpc-smoke")]
struct Args {
    /// Use the already-built sibling binaries (for controlled comparisons).
    #[arg(long, global = true)]
    skip_build: bool,

    #[command(subcommand)]
    scenario: Scenario,

    /// Override test duration per client (seconds)
    #[arg(long, default_value = "5", global = true)]
    duration: u64,

    /// Connect to an existing remote server instead of spawning one locally.
    /// Format: host:port (e.g. 10.0.1.5:9876)
    #[arg(long, global = true)]
    server_addr: Option<String>,

    /// Pool type: "fixed" (FixedSerialPool), "dynamic" (DynamicPool), "cpu" (CpuSetPool)
    #[arg(long, global = true)]
    pool: Option<String>,

    /// Enable tracing subscriber on server and client (info, debug)
    #[arg(long, global = true)]
    tracing: Option<String>,

    /// Emit machine-readable result JSON on stdout.
    #[arg(long, global = true)]
    json: bool,
}

impl Args {
    // Preserve the result schema for historical comparisons. A remote server's
    // mode is unknown; locally built servers always use ready-inline dispatch.
    fn recorded_inline_ready_dispatch(&self) -> Option<bool> {
        self.server_addr.is_none().then_some(true)
    }

    fn dispatch_mode_source(&self) -> Option<DispatchModeSource> {
        self.server_addr
            .is_none()
            .then_some(DispatchModeSource::Configured)
    }
}

#[derive(Subcommand)]
enum Scenario {
    /// Run all hogd service pairings simultaneously against one server.
    /// Models the full production topology: coordinator + gateways + executors + db + auth.
    Realistic {
        /// Number of gateway clients (default 3)
        #[arg(long, default_value = "3")]
        gateways: usize,
    },
    /// Pure connection scaling sweep — echo handler, increasing connection count.
    /// Finds the ceiling where tokio degrades.
    Scaling {
        /// Comma-separated connection counts to test
        #[arg(long, default_value = "10,50,100,200,500")]
        counts: String,
    },
    /// Run a single named profile.
    Single {
        /// Profile name
        profile: String,
        /// Override connection count
        #[arg(long)]
        connections: Option<usize>,
        /// Override maximum in-flight RPCs
        #[arg(long)]
        max_inflight: Option<usize>,
    },
    /// Multi-process throughput test — N client processes each with M connections,
    /// all driving echo RPCs against one server. Tests server-side scaling.
    Throughput {
        /// Number of client processes
        #[arg(long, default_value = "1,2,4,8")]
        clients: String,
        /// Connections per client process
        #[arg(long, default_value = "10")]
        connections_per_client: usize,
        /// Max in-flight RPCs per client process
        #[arg(long, default_value = "1000")]
        max_inflight: usize,
    },
}

/// Read the RSS (Resident Set Size) of a process from /proc/{pid}/status.
/// Returns the RSS in kilobytes, or None if unavailable (non-Linux).
fn read_rss_kb(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb: u64 = rest.trim().strip_suffix("kB")?.trim().parse().ok()?;
            return Some(kb);
        }
    }
    None
}

fn find_binary(name: &str) -> std::path::PathBuf {
    // Find sibling binary relative to the current executable.
    // Works regardless of working directory or debug/release mode.
    std::env::current_exe()
        .expect("cannot determine current exe path")
        .parent()
        .expect("exe has no parent dir")
        .join(name)
}

/// Server handle — either a locally-spawned process or a remote address.
enum ServerHandle {
    Local {
        child: tokio::process::Child,
        addr: std::net::SocketAddr,
        pid: u32,
        initial_rss_kb: Option<u64>,
    },
    Remote {
        addr: std::net::SocketAddr,
    },
}

impl ServerHandle {
    fn addr(&self) -> std::net::SocketAddr {
        match self {
            ServerHandle::Local { addr, .. } | ServerHandle::Remote { addr } => *addr,
        }
    }

    async fn kill(&mut self) -> Result<(), std::io::Error> {
        if let ServerHandle::Local { child, .. } = self {
            child.kill().await?;
        }
        Ok(())
    }

    fn report_rss(&self) {
        if let ServerHandle::Local {
            pid,
            initial_rss_kb,
            ..
        } = self
        {
            let final_rss = read_rss_kb(*pid);
            if let (Some(initial), Some(final_kb)) = (initial_rss_kb, final_rss) {
                let delta = final_kb as i64 - *initial as i64;
                let sign = if delta >= 0 { "+" } else { "" };
                eprintln!("[harness] server RSS: {initial} KB → {final_kb} KB ({sign}{delta} KB)",);
            }
        }
    }
}

/// Connect to an existing remote server or spawn a local one.
async fn get_server(
    remote_addr: &Option<String>,

    tracing_level: &Option<String>,
) -> Result<ServerHandle, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(addr_str) = remote_addr {
        let addr: std::net::SocketAddr = addr_str
            .parse()
            .map_err(|e| format!("invalid --server-addr '{addr_str}': {e}"))?;
        eprintln!("[harness] using remote server at {addr}");
        return Ok(ServerHandle::Remote { addr });
    }
    start_local_server(tracing_level).await
}

async fn start_local_server(
    tracing_level: &Option<String>,
) -> Result<ServerHandle, Box<dyn std::error::Error + Send + Sync>> {
    let bin = find_binary("rpc-smoke-server");
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(["--port", "0"]);
    if let Some(level) = tracing_level {
        cmd.args(["--tracing", level]);
    }
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;

    // Read the READY line from stdout (with timeout to avoid hanging if server crashes)
    let stdout = child.stdout.take().unwrap();
    let mut reader = tokio::io::BufReader::new(stdout);
    let mut line = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line),
    )
    .await
    .map_err(|_| "server did not print READY within 10s")??;

    let addr: std::net::SocketAddr = line
        .trim()
        .strip_prefix("READY ")
        .ok_or("server did not print READY")?
        .parse()?;

    let pid = child.id().unwrap_or(0);
    // Brief pause to let server stabilize before measuring baseline RSS
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let initial_rss_kb = read_rss_kb(pid);

    Ok(ServerHandle::Local {
        child,
        addr,
        pid,
        initial_rss_kb,
    })
}

struct ClientRunOptions<'a> {
    connections: Option<usize>,
    duration: u64,
    max_inflight: Option<usize>,
    pool_type: Option<&'a str>,
    tracing_level: Option<&'a str>,
    server_inline_ready_dispatch: Option<bool>,
    server_dispatch_mode_source: Option<DispatchModeSource>,
}

async fn run_client(
    addr: std::net::SocketAddr,
    profile: &str,
    options: ClientRunOptions<'_>,
) -> Result<ClientMetrics, Box<dyn std::error::Error + Send + Sync>> {
    let bin = find_binary("rpc-smoke-client");
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args([
        "--addr",
        &addr.to_string(),
        "--profile",
        profile,
        "--duration",
        &options.duration.to_string(),
    ]);
    if let Some(c) = options.connections {
        cmd.args(["--connections", &c.to_string()]);
    }
    if let Some(m) = options.max_inflight {
        cmd.args(["--max-inflight", &m.to_string()]);
    }
    if let Some(p) = options.pool_type {
        cmd.args(["--pool", p]);
    }
    if let Some(t) = options.tracing_level {
        cmd.args(["--tracing", t]);
    }

    let output = cmd.output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("client '{profile}' failed: {stderr}").into());
    }

    let stdout = String::from_utf8(output.stdout)?;
    let mut metrics: ClientMetrics = serde_json::from_str(stdout.trim())?;
    metrics.server_inline_ready_dispatch = options.server_inline_ready_dispatch;
    metrics.server_dispatch_mode_source = options.server_dispatch_mode_source;
    Ok(metrics)
}

/// Run a multi-tier sweep, managing server lifecycle per tier (local) or
/// reusing one server (remote). The callback receives (tier_index, server_addr)
/// and returns a Vec of results for that tier.
async fn run_sweep<F, Fut>(
    args: &Args,
    num_tiers: usize,
    mut tier_fn: F,
) -> Result<Vec<ClientMetrics>, Box<dyn std::error::Error + Send + Sync>>
where
    F: FnMut(usize, std::net::SocketAddr) -> Fut,
    Fut: std::future::Future<
            Output = Result<Vec<ClientMetrics>, Box<dyn std::error::Error + Send + Sync>>,
        >,
{
    let remote = args.server_addr.is_some();
    let shared_server = if remote {
        Some(get_server(&args.server_addr, &args.tracing).await?)
    } else {
        None
    };

    let mut all_results = Vec::new();

    for tier in 0..num_tiers {
        let mut local_server = if shared_server.is_none() {
            Some(get_server(&args.server_addr, &args.tracing).await?)
        } else {
            None
        };
        let server = shared_server.as_ref().or(local_server.as_ref()).unwrap();

        let tier_results = tier_fn(tier, server.addr()).await?;
        all_results.extend(tier_results);

        if let Some(ref mut s) = local_server {
            s.report_rss();
            s.kill().await?;
        }
    }

    if let Some(ref s) = shared_server {
        s.report_rss();
    }

    Ok(all_results)
}

fn print_summary(results: &[ClientMetrics]) {
    if let Some(mode) = results
        .first()
        .and_then(|metrics| metrics.server_inline_ready_dispatch)
    {
        let source = match results
            .first()
            .and_then(|metrics| metrics.server_dispatch_mode_source)
        {
            Some(DispatchModeSource::Configured) => "configured locally",
            Some(DispatchModeSource::Declared) => "declared remote mode",
            None => "unknown source",
        };
        println!("\nServer inline-ready dispatch: {mode} ({source})");
    }
    println!(
        "\n{:<15} {:>6} {:>10} {:>10} {:>10} {:>10} {:>8}",
        "Profile", "Conns", "RPCs", "Errors", "p50 µs", "p99 µs", "RPS"
    );
    println!("{:-<80}", "");
    for m in results {
        println!(
            "{:<15} {:>6} {:>10} {:>10} {:>10.0} {:>10.0} {:>8.0}",
            m.profile_name,
            m.connections,
            m.total_rpcs,
            m.total_errors,
            m.latency_us.p50,
            m.latency_us.p99,
            m.throughput_rps
        );
    }
    println!("{:-<80}", "");

    let total_conns: usize = results.iter().map(|m| m.connections).sum();
    let total_rpcs: u64 = results.iter().map(|m| m.total_rpcs).sum();
    let total_errors: u64 = results.iter().map(|m| m.total_errors).sum();
    let max_p99 = results
        .iter()
        .map(|m| m.latency_us.p99)
        .fold(0.0f64, f64::max);
    let agg_rps: f64 = results.iter().map(|m| m.throughput_rps).sum();

    println!("Total: {total_conns} connections, {total_rpcs} RPCs, {total_errors} errors");
    println!("Aggregate throughput: {agg_rps:.0} RPC/s");
    println!("Max p99 latency: {max_p99:.0} µs");

    if total_errors > 0 {
        println!("\nRESULT: FAIL ({total_errors} errors)");
    } else {
        println!("\nRESULT: PASS");
    }
}

fn emit_results(results: &[ClientMetrics], json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(results).unwrap());
    } else {
        print_summary(results);
    }
}

fn has_failures(metrics: &ClientMetrics) -> bool {
    metrics.total_errors > 0
        || metrics.rpc_counts.timed_out > 0
        || metrics.rpc_counts.outstanding > 0
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = Args::parse();

    // Build binaries in release mode for meaningful performance numbers
    if !args.skip_build {
        eprintln!("[harness] building binaries (release)...");
        let status = tokio::process::Command::new("cargo")
            .args(["build", "--release", "-p", "rpc-smoke"])
            .status()
            .await?;
        if !status.success() {
            return Err("cargo build failed".into());
        }
    }

    if let Some(ref pool) = args.pool {
        eprintln!("[harness] pool: {pool}");
    }

    match &args.scenario {
        Scenario::Realistic { gateways } => {
            let gateways = *gateways;
            eprintln!(
                "[harness] scenario: realistic (coordinator + {gateways} gateways + 4 executors + 4 db-shards + auth)"
            );
            let mut server = get_server(&args.server_addr, &args.tracing).await?;
            let addr = server.addr();

            // Build list of (profile, connections_override) for all clients
            let mut clients: Vec<(&str, Option<usize>)> = Vec::new();
            clients.push(("coordinator", None));
            for _ in 0..gateways {
                clients.push(("gateway", None));
            }
            for _ in 0..4 {
                clients.push(("executor", None));
            }
            for _ in 0..4 {
                clients.push(("db-shard", None));
            }
            clients.push(("auth", None));

            // Launch all clients concurrently via JoinSet
            let mut tasks = tokio::task::JoinSet::new();
            for (profile, conns) in clients {
                let duration = args.duration;
                let pool = args.pool.clone();
                let tracing_level = args.tracing.clone();
                let inline_ready_dispatch = args.recorded_inline_ready_dispatch();
                let dispatch_mode_source = args.dispatch_mode_source();
                tasks.spawn(async move {
                    run_client(
                        addr,
                        profile,
                        ClientRunOptions {
                            connections: conns,
                            duration,
                            max_inflight: None,
                            pool_type: pool.as_deref(),
                            tracing_level: tracing_level.as_deref(),
                            server_inline_ready_dispatch: inline_ready_dispatch,
                            server_dispatch_mode_source: dispatch_mode_source,
                        },
                    )
                    .await
                });
            }

            let mut results = Vec::new();
            while let Some(res) = tasks.join_next().await {
                results.push(res??);
            }

            emit_results(&results, args.json);
            server.report_rss();
            server.kill().await?;

            let has_errors = results.iter().any(has_failures);
            std::process::exit(if has_errors { 1 } else { 0 });
        }

        Scenario::Scaling { counts } => {
            let counts: Vec<usize> = counts
                .split(',')
                .map(|s| s.trim().parse().expect("invalid count"))
                .collect();

            eprintln!("[harness] scenario: scaling sweep ({counts:?} connections)");

            let results = run_sweep(&args, counts.len(), |tier, server_addr| {
                let count = counts[tier];
                let duration = args.duration;
                let pool = args.pool.clone();
                let tracing_level = args.tracing.clone();
                let inline_ready_dispatch = args.recorded_inline_ready_dispatch();
                let dispatch_mode_source = args.dispatch_mode_source();
                eprintln!("[harness] testing {count} connections...");
                async move {
                    let result = run_client(
                        server_addr,
                        "echo",
                        ClientRunOptions {
                            connections: Some(count),
                            duration,
                            max_inflight: None,
                            pool_type: pool.as_deref(),
                            tracing_level: tracing_level.as_deref(),
                            server_inline_ready_dispatch: inline_ready_dispatch,
                            server_dispatch_mode_source: dispatch_mode_source,
                        },
                    )
                    .await?;
                    Ok(vec![result])
                }
            })
            .await?;

            emit_results(&results, args.json);

            let has_errors = results.iter().any(has_failures);
            std::process::exit(if has_errors { 1 } else { 0 });
        }

        Scenario::Single {
            profile,
            connections,
            max_inflight,
        } => {
            eprintln!("[harness] scenario: single profile '{profile}'");
            let mut server = get_server(&args.server_addr, &args.tracing).await?;

            let result = run_client(
                server.addr(),
                profile,
                ClientRunOptions {
                    connections: *connections,
                    duration: args.duration,
                    max_inflight: *max_inflight,
                    pool_type: args.pool.as_deref(),
                    tracing_level: args.tracing.as_deref(),
                    server_inline_ready_dispatch: args.recorded_inline_ready_dispatch(),
                    server_dispatch_mode_source: args.dispatch_mode_source(),
                },
            )
            .await?;
            let has_errors = has_failures(&result);
            emit_results(&[result], args.json);

            server.report_rss();
            server.kill().await?;

            std::process::exit(if has_errors { 1 } else { 0 });
        }

        Scenario::Throughput {
            clients,
            connections_per_client,
            max_inflight,
        } => {
            let connections_per_client = *connections_per_client;
            let max_inflight = *max_inflight;
            let client_counts: Vec<usize> = clients
                .split(',')
                .map(|s| s.trim().parse().expect("invalid client count"))
                .collect();

            eprintln!(
                "[harness] scenario: throughput sweep ({client_counts:?} clients × {connections_per_client} conns, {max_inflight} max-inflight/client)"
            );

            let results = run_sweep(&args, client_counts.len(), |tier, server_addr| {
                let num_clients = client_counts[tier];
                let duration = args.duration;
                let conns = connections_per_client;
                let inflight = max_inflight;
                let pool = args.pool.clone();
                let tracing_level = args.tracing.clone();
                let inline_ready_dispatch = args.recorded_inline_ready_dispatch();
                let dispatch_mode_source = args.dispatch_mode_source();
                eprintln!("[harness] testing {num_clients} client processes...");
                async move {
                    let mut tasks = tokio::task::JoinSet::new();
                    for i in 0..num_clients {
                        let label = format!("echo-{num_clients}x{conns}#{i}");
                        let pool = pool.clone();
                        let tracing_level = tracing_level.clone();
                        tasks.spawn(async move {
                            run_client(
                                server_addr,
                                &label,
                                ClientRunOptions {
                                    connections: Some(conns),
                                    duration,
                                    max_inflight: Some(inflight),
                                    pool_type: pool.as_deref(),
                                    tracing_level: tracing_level.as_deref(),
                                    server_inline_ready_dispatch: inline_ready_dispatch,
                                    server_dispatch_mode_source: dispatch_mode_source,
                                },
                            )
                            .await
                        });
                    }

                    let mut tier_results = Vec::new();
                    while let Some(res) = tasks.join_next().await {
                        tier_results.push(res??);
                    }

                    // Aggregate into a single row for this tier
                    let total_rpcs: u64 = tier_results.iter().map(|m| m.total_rpcs).sum();
                    let total_errors: u64 = tier_results.iter().map(|m| m.total_errors).sum();
                    let rpc_counts =
                        tier_results
                            .iter()
                            .fold(RpcCounts::default(), |mut acc, m| {
                                acc.submitted += m.rpc_counts.submitted;
                                acc.completed += m.rpc_counts.completed;
                                acc.failed += m.rpc_counts.failed;
                                acc.timed_out += m.rpc_counts.timed_out;
                                acc.outstanding += m.rpc_counts.outstanding;
                                acc
                            });
                    let agg_rps: f64 = tier_results.iter().map(|m| m.throughput_rps).sum();
                    let max_wall = tier_results
                        .iter()
                        .map(|m| m.wall_time_ms)
                        .fold(0.0f64, f64::max);
                    let max_p50 = tier_results
                        .iter()
                        .map(|m| m.latency_us.p50)
                        .fold(0.0f64, f64::max);
                    let max_p95 = tier_results
                        .iter()
                        .map(|m| m.latency_us.p95)
                        .fold(0.0f64, f64::max);
                    let max_p99 = tier_results
                        .iter()
                        .map(|m| m.latency_us.p99)
                        .fold(0.0f64, f64::max);
                    let max_max = tier_results
                        .iter()
                        .map(|m| m.latency_us.max)
                        .fold(0.0f64, f64::max);
                    let min_min = tier_results
                        .iter()
                        .map(|m| m.latency_us.min)
                        .fold(f64::MAX, f64::min);
                    let mean = if rpc_counts.completed > 0 {
                        tier_results
                            .iter()
                            .map(|m| m.latency_us.mean * m.rpc_counts.completed as f64)
                            .sum::<f64>()
                            / rpc_counts.completed as f64
                    } else {
                        0.0
                    };

                    let total_conns = num_clients * conns;
                    Ok(vec![metrics::ClientMetrics {
                        profile_name: format!("{num_clients}×{conns}"),
                        connections: total_conns,
                        server_inline_ready_dispatch: inline_ready_dispatch,
                        server_dispatch_mode_source: dispatch_mode_source,
                        rpc_counts,
                        run_config: tier_results
                            .first()
                            .and_then(|metrics| metrics.run_config.clone())
                            .map(|mut config| {
                                config.client_processes = num_clients;
                                config
                            }),
                        total_rpcs,
                        total_errors,
                        wall_time_ms: max_wall,
                        throughput_rps: agg_rps,
                        latency_us: metrics::LatencyStats {
                            min: min_min,
                            p50: max_p50,
                            p95: max_p95,
                            p99: max_p99,
                            max: max_max,
                            mean,
                        },
                    }])
                }
            })
            .await?;

            emit_results(&results, args.json);

            let has_errors = results.iter().any(has_failures);
            std::process::exit(if has_errors { 1 } else { 0 });
        }
    }
}
