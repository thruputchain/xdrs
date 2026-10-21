#![allow(dead_code, unreachable_pub)]

use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchModeSource {
    Configured,
    Declared,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct RpcCounts {
    pub submitted: u64,
    pub completed: u64,
    pub failed: u64,
    pub timed_out: u64,
    pub outstanding: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClientRunConfig {
    pub server_addr: String,
    pub pool_type: String,
    pub client_processes: usize,
    pub max_inflight_per_process: usize,
    pub duration_secs: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClientMetrics {
    pub profile_name: String,
    pub connections: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_inline_ready_dispatch: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_dispatch_mode_source: Option<DispatchModeSource>,
    #[serde(default)]
    pub rpc_counts: RpcCounts,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_config: Option<ClientRunConfig>,
    pub total_rpcs: u64,
    pub total_errors: u64,
    pub wall_time_ms: f64,
    pub throughput_rps: f64,
    pub latency_us: LatencyStats,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LatencyStats {
    pub min: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
    pub mean: f64,
}

impl ClientMetrics {
    /// Compute metrics from a vec of per-call latencies.
    pub fn from_samples(
        profile_name: String,
        connections: usize,
        wall_time: Duration,
        samples: &mut [Duration],
        rpc_counts: RpcCounts,
    ) -> Self {
        samples.sort_unstable();
        debug_assert_eq!(samples.len() as u64, rpc_counts.completed);
        debug_assert_eq!(
            rpc_counts.submitted,
            rpc_counts.completed
                + rpc_counts.failed
                + rpc_counts.timed_out
                + rpc_counts.outstanding
        );
        let total_rpcs = rpc_counts.submitted;
        let total_errors = rpc_counts.failed + rpc_counts.timed_out + rpc_counts.outstanding;
        let wall_ms = wall_time.as_secs_f64() * 1000.0;
        let throughput = if wall_ms > 0.0 {
            samples.len() as f64 / wall_time.as_secs_f64()
        } else {
            0.0
        };

        let latency_us = if samples.is_empty() {
            LatencyStats {
                min: 0.0,
                p50: 0.0,
                p95: 0.0,
                p99: 0.0,
                max: 0.0,
                mean: 0.0,
            }
        } else {
            let us = |d: Duration| d.as_secs_f64() * 1_000_000.0;
            let total: Duration = samples.iter().sum();
            LatencyStats {
                min: us(samples[0]),
                p50: us(percentile(samples, 50.0)),
                p95: us(percentile(samples, 95.0)),
                p99: us(percentile(samples, 99.0)),
                max: us(*samples.last().unwrap()),
                mean: total.as_secs_f64() * 1_000_000.0 / samples.len() as f64,
            }
        };

        Self {
            profile_name,
            connections,
            server_inline_ready_dispatch: None,
            server_dispatch_mode_source: None,
            rpc_counts,
            run_config: None,
            total_rpcs,
            total_errors,
            wall_time_ms: wall_ms,
            throughput_rps: throughput,
            latency_us,
        }
    }
}

fn percentile(sorted: &[Duration], pct: f64) -> Duration {
    let idx = ((sorted.len() as f64 * pct / 100.0).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len() - 1);
    sorted[idx]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_counts_remain_in_legacy_totals() {
        let counts = RpcCounts {
            submitted: 10,
            timed_out: 10,
            ..RpcCounts::default()
        };
        let metrics = ClientMetrics::from_samples(
            "timeout".to_string(),
            1,
            Duration::from_secs(1),
            &mut [],
            counts,
        );

        assert_eq!(metrics.total_rpcs, 10);
        assert_eq!(metrics.total_errors, 10);
        assert_eq!(metrics.rpc_counts.timed_out, 10);
    }
}
