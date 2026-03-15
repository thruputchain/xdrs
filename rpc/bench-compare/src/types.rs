#![allow(unreachable_pub)]
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct BenchResult {
    pub metadata: Metadata,
    pub scenarios: Vec<ScenarioResult>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Metadata {
    pub git_sha: String,
    pub timestamp: String,
    pub feature_flags: Vec<String>,
    pub hostname: String,
    pub rpc_driver_version: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScenarioResult {
    pub name: String,
    pub params: BTreeMap<String, serde_json::Value>,
    pub samples: usize,
    pub timed_out: bool,
    pub latency_us: Option<LatencyStats>,
    pub wall_time_ms: Option<f64>,
    pub throughput_calls_per_sec: Option<f64>,
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
