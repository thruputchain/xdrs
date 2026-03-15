# rpc-smoke

Multi-connection smoke test for `rpc_driver`. Validates concurrent dispatch under realistic hogd service topology workloads.

## Quick Start

```sh
cargo build --release -p rpc-smoke

# Run all hogd pairings (coordinator + gateways + executors + db-shards + auth)
cargo run --release -p rpc-smoke --bin rpc-smoke -- realistic --duration 10

# Multi-process throughput test
cargo run --release -p rpc-smoke --bin rpc-smoke -- throughput --clients 1,4,8,16 --duration 10
```

## Binaries

| Binary | Purpose |
|---|---|
| `rpc-smoke-server` | RPC server with configurable handler profiles (echo, sleep, mutex, CPU spin) |
| `rpc-smoke-client` | Connects via FixedSerialPool, DynamicPool, or CpuSetPool; drives RPCs; outputs JSON metrics |
| `rpc-smoke` | Harness that orchestrates server + N client processes |

## Client Options

| Flag | Default | Description |
|---|---|---|
| `--addr` | (required) | Server address (host:port) |
| `--profile` | (required) | Profile name: coordinator, gateway, executor, db-shard, auth, echo |
| `--connections` | profile default | Connection count (FixedSerialPool pool size, DynamicPool max) |
| `--duration` | 5 | Test duration in seconds |
| `--max-inflight` | 100 | Max concurrent in-flight RPCs |
| `--pool` | fixed | Pool type: `fixed`, `dynamic`, or `cpu` |
| `--tracing` | (none) | Enable tracing subscriber at given level (info, debug) |

## Pool Types

| Pool | Per-conn concurrency | Routing | Models |
|---|---|---|---|
| `fixed` | 1 in-flight (serial) | FIFO via channel | x23 sync RPC (executor→DB) |
| `dynamic` | Many in-flight | Min-pending load balance | x23 async RPC (coordinator→mempool) |
| `cpu` | Many in-flight | LLC-affine (sched_getcpu) | x23 RpcMulticlient / PerCpuSet |

## Harness Options

| Flag | Default | Description |
|---|---|---|
| `--duration` | 5 | Test duration per client (seconds) |
| `--server-max-concurrent` | 64 | Server concurrency limit (ignored with `--server-addr`) |
| `--server-addr` | (spawn local) | Remote server address (host:port) |
| `--pool` | (none = fixed) | Pool type for all clients: `fixed`, `dynamic`, or `cpu` |
| `--tracing` | (none) | Enable tracing subscriber on server and client (info, debug) |

## Harness Scenarios

| Scenario | What it tests |
|---|---|
| `realistic` | All hogd pairings simultaneously (110 connections, 13 processes) |
| `scaling` | Single-client echo with increasing connection count |
| `throughput` | Multi-process echo to find server-side scaling ceiling |
| `single` | One named profile |

Server RSS is reported after each run for leak detection.

## EPYC Cluster Benchmark Playbook

All tests below assume server on machine A, client(s) on machine B.
Build on both machines first: `cargo build --release -p rpc-smoke`

Start the server once on A (all tests reuse it):

```sh
# A:
rpc-smoke-server --bind 0.0.0.0 --port 9876 --max-concurrent 256
```

### Full matrix run

Run every scenario × every pool type. This is the complete benchmark suite.

```sh
SERVER=A:9876
DUR=30

for POOL in fixed dynamic cpu; do
  echo "=== Pool: $POOL ==="

  # 1. Correctness — realistic smoke test
  rpc-smoke realistic --server-addr $SERVER --pool $POOL --duration $DUR

  # 2. Connection scaling
  rpc-smoke scaling --server-addr $SERVER --pool $POOL \
      --counts 10,50,100,200,500,1000 --duration $DUR

  # 3. Multi-process throughput
  rpc-smoke throughput --server-addr $SERVER --pool $POOL \
      --clients 1,2,4,8,16,32,64 \
      --connections-per-client 10 --max-inflight 2000 --duration $DUR
done
```

This produces a 3×3 matrix (3 scenarios × 3 pools) that directly answers:

| Question | Scenario | Compare across |
|---|---|---|
| Which pool is fastest for a single client? | scaling | pools at each connection count |
| Which pool scales best with more client processes? | throughput | pools at each client count |
| Does pool choice affect mixed-workload correctness? | realistic | pools (expect zero errors for all) |

### Individual tests

Run these separately if the full matrix is too long, or to drill into specific questions.

#### Handler profile stress

Do CPU-spin and mutex-contention handlers degrade under load?

```sh
for POOL in fixed dynamic cpu; do
  rpc-smoke single --server-addr $SERVER --pool $POOL executor --duration $DUR
  rpc-smoke single --server-addr $SERVER --pool $POOL coordinator --duration $DUR
done
```

#### Long soak — leak and stability

Any memory leaks, latency drift, or stalls over minutes?

```sh
# Realistic workload, 5 minutes per pool:
for POOL in fixed dynamic cpu; do
  rpc-smoke realistic --server-addr $SERVER --pool $POOL --duration 300
done

# Throughput saturation, 5 minutes:
rpc-smoke throughput --server-addr $SERVER --pool dynamic \
    --clients 16 --connections-per-client 10 \
    --max-inflight 2000 --duration 300
```

### How the pool types map to x23/hogd patterns

x23 has two RPC modes, each used by different service pairings:

- **Sync RPC** (`SRpcMulticlient`): 1 in-flight per connection, more connections.
  Used by executor→DB. Our **FixedSerialPool** models this — serial dispatch per
  connection, throughput scales with connection count.

- **Async RPC** (`RpcMulticlient` + `PerCpuSet`): many in-flight per connection,
  one connection per LLC, XID-multiplexed. Used by coordinator→mempool and
  anywhere x23 needs saturation. Our **DynamicPool** and **CpuSetPool** model
  this — concurrent in-flight per connection, throughput comes from
  multiplexing rather than connection count.

x23 achieves millions of RPCs/sec via thread-per-core architecture: each of 64
cores runs its own epoll+coroutine loop with zero cross-thread synchronization.
Our server uses tokio's work-stealing scheduler instead, which is simpler but
adds overhead from task migration and shared queues. **We expect the server to
be the bottleneck** — the multi-process throughput test is the key measurement:
it isolates server-side scaling by throwing independent client processes at it.

### What to look for

- **Pool comparison (scaling)**: DynamicPool should match or beat FixedSerialPool
  at low connection counts (many in-flight per conn vs serial). FixedSerialPool
  should catch up with more connections (x23 sync pattern). CpuSetPool's LLC
  benefit should be more visible on EPYC (8 LLCs) than desktop (2 LLCs).
- **Server scaling (throughput)**: this is the main event. Throughput should
  increase with client count up to core count, then plateau. On 64-core EPYC,
  we want to see how close we get to x23's millions/sec. Watch for throughput
  *decreasing* past a certain client count — that signals tokio work-stealing
  contention and is the ceiling we're trying to measure.
- **Cross-pool throughput comparison**: at the same client/connection count, how
  much does pool choice matter? If DynamicPool consistently beats FixedSerialPool
  under multi-process load, that tells us per-connection multiplexing is more
  efficient than connection count scaling for our server's write path.
- **Soak**: RSS should stabilize within the first few seconds. Throughput should
  not drift. p99 should not creep upward over time.
- **Handler stress**: CPU-spin profiles run via `spawn_blocking` to avoid
  starving tokio workers. Watch for latency inflation on other profiles running
  simultaneously.
