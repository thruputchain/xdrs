#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// RPC program number for the smoke test protocol ("SMOK" in ASCII).
pub const SMOKE_PROG: u32 = 0x534D4F4B;

/// Profile tag bytes — first byte of every RPC request payload.
/// The server reads this to decide how to simulate the handler.
pub const TAG_ECHO: u8 = 0x01;
pub const TAG_SLEEP: u8 = 0x02;
pub const TAG_MUTEX: u8 = 0x03;
pub const TAG_CPU_SPIN: u8 = 0x04;
pub const TAG_SLEEP_MUTEX: u8 = 0x05;

/// Encode a profile-tagged payload.
///
/// Layout: [tag: 1B] [duration_us: 4B big-endian] [padding: N bytes to reach `size`]
///
/// `duration_us` is interpreted differently per tag:
/// - TAG_SLEEP: sleep for this many microseconds
/// - TAG_MUTEX: hold mutex for this many microseconds
/// - TAG_CPU_SPIN: busy-spin for this many microseconds
/// - TAG_SLEEP_MUTEX: sleep then hold mutex, each for duration_us / 2
/// - TAG_ECHO: ignored (just echoes the payload)
pub fn encode_payload(tag: u8, duration_us: u32, total_size: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(total_size.max(5));
    buf.push(tag);
    buf.extend_from_slice(&duration_us.to_be_bytes());
    // Pad to desired total size
    if total_size > 5 {
        buf.resize(total_size, 0xAA);
    }
    buf
}

/// Decode tag and duration from a payload.
pub fn decode_payload(data: &[u8]) -> (u8, u32) {
    if data.is_empty() {
        return (TAG_ECHO, 0);
    }
    let tag = data[0];
    let duration_us = if data.len() >= 5 {
        u32::from_be_bytes([data[1], data[2], data[3], data[4]])
    } else {
        0
    };
    (tag, duration_us)
}

/// A named pairing profile with all parameters needed to drive a client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingProfile {
    pub name: String,
    /// Number of TCP connections (pool size; ignored by CpuSetPool)
    pub connections: usize,
    /// RPC payload size in bytes
    pub payload_bytes: usize,
    /// Handler profile tag
    pub tag: u8,
    /// Simulated handler duration in microseconds
    pub handler_duration_us: u32,
    /// Duration to run the test in seconds
    pub duration_secs: u64,
}

/// Built-in profiles modeled from hogd service pairings.
pub fn builtin_profiles() -> Vec<PairingProfile> {
    vec![
        PairingProfile {
            name: "coordinator".into(),
            connections: 20,
            payload_bytes: 1500,
            tag: TAG_SLEEP_MUTEX,
            handler_duration_us: 2000, // 2ms total (1ms sleep + 1ms mutex)
            duration_secs: 5,
        },
        PairingProfile {
            name: "gateway".into(),
            connections: 15,
            payload_bytes: 800,
            tag: TAG_SLEEP_MUTEX,
            handler_duration_us: 2000,
            duration_secs: 5,
        },
        PairingProfile {
            name: "executor".into(),
            connections: 10,
            payload_bytes: 200,
            tag: TAG_CPU_SPIN,
            handler_duration_us: 500, // 500µs CPU work
            duration_secs: 5,
        },
        PairingProfile {
            name: "db-shard".into(),
            connections: 1,
            payload_bytes: 1000,
            tag: TAG_SLEEP,
            handler_duration_us: 2000, // 2ms I/O
            duration_secs: 5,
        },
        PairingProfile {
            name: "auth".into(),
            connections: 1,
            payload_bytes: 500,
            tag: TAG_CPU_SPIN,
            handler_duration_us: 200, // 200µs CPU
            duration_secs: 5,
        },
        PairingProfile {
            name: "echo".into(),
            connections: 1, // overridden by scaling sweep
            payload_bytes: 64,
            tag: TAG_ECHO,
            handler_duration_us: 0,
            duration_secs: 5,
        },
    ]
}
