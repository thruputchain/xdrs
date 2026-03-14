#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Serialize, Deserialize)]
pub struct ClientMetrics {
    pub profile_name: String,
    pub connections: usize,
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
        errors: u64,
        wall_time: Duration,
        samples: &mut [Duration],
    ) -> Self {
        samples.sort_unstable();
        let total_rpcs = samples.len() as u64 + errors;
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
            total_rpcs,
            total_errors: errors,
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
