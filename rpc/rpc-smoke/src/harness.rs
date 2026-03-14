mod metrics;
mod profile;

use std::process::Stdio;

use clap::{Parser, Subcommand};
use metrics::ClientMetrics;

#[derive(Parser)]
#[command(name = "rpc-smoke")]
struct Args {
    #[command(subcommand)]
    scenario: Scenario,

    /// Override test duration per client (seconds)
    #[arg(long, default_value = "5", global = true)]
    duration: u64,

    /// Max concurrent dispatches on the server (ignored with --server-addr)
    #[arg(long, default_value = "64", global = true)]
    server_max_concurrent: usize,

    /// Connect to an existing remote server instead of spawning one locally.
    /// Format: host:port (e.g. 10.0.1.5:9876)
    #[arg(long, global = true)]
    server_addr: Option<String>,

    /// Pool type: "fixed" (FixedSerialPool), "dynamic" (DynamicPool), "cpu" (CpuSetPool)
    #[arg(long, global = true)]
    pool: Option<String>,
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
    max_concurrent: usize,
) -> Result<ServerHandle, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(addr_str) = remote_addr {
        let addr: std::net::SocketAddr = addr_str
            .parse()
            .map_err(|e| format!("invalid --server-addr '{addr_str}': {e}"))?;
        eprintln!("[harness] using remote server at {addr}");
        return Ok(ServerHandle::Remote { addr });
    }
    start_local_server(max_concurrent).await
}

async fn start_local_server(
    max_concurrent: usize,
) -> Result<ServerHandle, Box<dyn std::error::Error + Send + Sync>> {
    let bin = find_binary("rpc-smoke-server");
    let mut child = tokio::process::Command::new(bin)
        .args([
            "--port",
            "0",
            "--max-concurrent",
            &max_concurrent.to_string(),
        ])
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

async fn run_client(
    addr: std::net::SocketAddr,
    profile: &str,
    connections: Option<usize>,
    duration: u64,
    max_inflight: Option<usize>,
    pool_type: Option<&str>,
) -> Result<ClientMetrics, Box<dyn std::error::Error + Send + Sync>> {
    let bin = find_binary("rpc-smoke-client");
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args([
        "--addr",
        &addr.to_string(),
        "--profile",
        profile,
        "--duration",
        &duration.to_string(),
    ]);
    if let Some(c) = connections {
        cmd.args(["--connections", &c.to_string()]);
    }
    if let Some(m) = max_inflight {
        cmd.args(["--max-inflight", &m.to_string()]);
    }
    if let Some(p) = pool_type {
        cmd.args(["--pool", p]);
    }

    let output = cmd.output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("client '{profile}' failed: {stderr}").into());
    }

    let stdout = String::from_utf8(output.stdout)?;
    let metrics: ClientMetrics = serde_json::from_str(stdout.trim())?;
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
        Some(get_server(&args.server_addr, args.server_max_concurrent).await?)
    } else {
        None
    };

    let mut all_results = Vec::new();

    for tier in 0..num_tiers {
        let mut local_server = if shared_server.is_none() {
            Some(get_server(&args.server_addr, args.server_max_concurrent).await?)
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = Args::parse();

    // Build binaries in release mode for meaningful performance numbers
    eprintln!("[harness] building binaries (release)...");
    let status = std::process::Command::new("cargo")
        .args(["build", "--release", "-p", "rpc-smoke"])
        .status()?;
    if !status.success() {
        return Err("cargo build failed".into());
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
            let mut server = get_server(&args.server_addr, args.server_max_concurrent).await?;
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
                tasks.spawn(async move {
                    run_client(addr, profile, conns, duration, None, pool.as_deref()).await
                });
            }

            let mut results = Vec::new();
            while let Some(res) = tasks.join_next().await {
                results.push(res??);
            }

            print_summary(&results);
            server.report_rss();
            server.kill().await?;

            let has_errors = results.iter().any(|m| m.total_errors > 0);
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
                eprintln!("[harness] testing {count} connections...");
                async move {
                    let result =
                        run_client(server_addr, "echo", Some(count), duration, None, pool.as_deref()).await?;
                    Ok(vec![result])
                }
            })
            .await?;

            print_summary(&results);

            let has_errors = results.iter().any(|m| m.total_errors > 0);
            std::process::exit(if has_errors { 1 } else { 0 });
        }

        Scenario::Single {
            profile,
            connections,
        } => {
            eprintln!("[harness] scenario: single profile '{profile}'");
            let mut server = get_server(&args.server_addr, args.server_max_concurrent).await?;

            let result = run_client(
                server.addr(),
                profile,
                *connections,
                args.duration,
                None,
                args.pool.as_deref(),
            )
            .await?;
            let has_errors = result.total_errors > 0;
            print_summary(&[result]);

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
                eprintln!("[harness] testing {num_clients} client processes...");
                async move {
                    let mut tasks = tokio::task::JoinSet::new();
                    for i in 0..num_clients {
                        let label = format!("echo-{num_clients}x{conns}#{i}");
                        let pool = pool.clone();
                        tasks.spawn(async move {
                            run_client(
                                server_addr,
                                &label,
                                Some(conns),
                                duration,
                                Some(inflight),
                                pool.as_deref(),
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
                    let mean = if total_rpcs > 0 {
                        tier_results
                            .iter()
                            .map(|m| m.latency_us.mean * m.total_rpcs as f64)
                            .sum::<f64>()
                            / total_rpcs as f64
                    } else {
                        0.0
                    };

                    let total_conns = num_clients * conns;
                    Ok(vec![metrics::ClientMetrics {
                        profile_name: format!("{num_clients}×{conns}"),
                        connections: total_conns,
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

            print_summary(&results);

            let has_errors = results.iter().any(|m| m.total_errors > 0);
            std::process::exit(if has_errors { 1 } else { 0 });
        }
    }
}
