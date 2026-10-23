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
| `--profile` | (required) | Profile name: coordinator, gateway, executor, db-shard, auth, echo, cpu-prefix-ready |
| `--connections` | profile default | Connection count (FixedSerialPool pool size, DynamicPool max) |
| `--duration` | 5 | Test duration in seconds |
| `--max-inflight` | 100 | Max concurrent in-flight RPCs |
| `--pool` | fixed | Pool type: `fixed`, `dynamic`, `cpu`, or `multiplex` |
| `--tracing` | (none) | Enable tracing subscriber at given level (info, debug) |

The server always runs ready handlers inline, spawns suspended handlers, and
batches replies automatically at read-buffer boundaries. No dispatch flags are
needed. Configure application load with the client's `--max-inflight` option.

## Pool Types

| Pool | Per-conn concurrency | Routing | Models |
|---|---|---|---|
| `fixed` | 1 in-flight (serial) | FIFO via channel | x23 sync RPC (executor→DB) |
| `dynamic` | Many in-flight | Min-pending load balance | x23 async RPC (coordinator→mempool) |
| `cpu` | Many in-flight | LLC-affine (sched_getcpu) | x23 RpcMulticlient / PerCpuSet |
| `multiplex` | Many in-flight | Least in flight, fixed set | x23 async RPC over N connections |

## Harness Options

| Flag | Default | Description |
|---|---|---|
| `--duration` | 5 | Test duration per client (seconds) |
| `--server-addr` | (spawn local) | Remote server address (host:port) |
| `--pool` | (none = fixed) | Pool type for all clients: `fixed`, `dynamic`, `cpu`, or `multiplex` |
| `--tracing` | (none) | Enable tracing subscriber on server and client (info, debug) |
| `--json` | false | Emit machine-readable metrics, including dispatch mode and completion counts |

## Harness Scenarios

| Scenario | What it tests |
|---|---|
| `realistic` | All hogd pairings simultaneously (110 connections, 13 processes) |
| `scaling` | Single-client echo with increasing connection count |
| `throughput` | Multi-process echo to find server-side scaling ceiling |
| `single` | One named profile |

Server RSS is reported after each run for leak detection.

With `--json`, each result records whether the server dispatch mode was
configured locally or declared for a remote server, plus submitted, completed,
failed, timed-out, and outstanding RPC counts. A successful run satisfies
`submitted = completed + failed + timed_out + outstanding` with the last three
counts at zero.

The `cpu-prefix-ready` profile is an adverse control for inline dispatch. Use a
multiplexing pool so multiple CALLs reach one connection concurrently:

```sh
rpc-smoke single cpu-prefix-ready --pool dynamic --connections 1 \
    --max-inflight 10
```

## Tracing

Pass `--tracing info` or `--tracing debug` to enable structured tracing output on server and client processes. The subscriber setup in `src/server.rs` serves as a reference example for consumers of `rpc_driver`.

Key subscriber decisions (see `src/server.rs` inline docs):
- `FmtSpan::CLOSE` — emits `time.busy` (handler execution) and `time.idle` (scheduling gap) on span close, which is the most useful output for performance analysis
- Writes to stderr so it doesn't interfere with JSON metrics on stdout
- Level filter: `info` shows connection lifecycle only; `debug` adds per-RPC spans

Example output at DEBUG level:

```
rpc_connection{peer=192.0.2.1:4567 role="server"}:rpc_call{xid=42 prog=100 vers=1 proc=1}: close time.busy=4.2us time.idle=515us
```

Performance impact: INFO adds <1% overhead; DEBUG adds ~5% on realistic workloads.

## EPYC Cluster Benchmark Playbook

All tests below assume server on machine A, client(s) on machine B.
Build on both machines first: `cargo build --release -p rpc-smoke`

Start the server once on A (all tests reuse it):

```sh
# A:
rpc-smoke-server --bind 0.0.0.0 --port 9876
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

### Full runtime evaluation

`python3 rpc/scripts/evaluate_runtimes.py --output benchmark-results/evaluation`
(from the repository root) builds the baseline (where the checkout forked from
`main`, or `--baseline <rev>`) and the current checkout in separate Cargo target
directories. It runs identical benchmark sources against both drivers; each
keeps its own service implementations so a handler API change still builds.
Every run also includes a candidate control that reruns the candidate's own
executables, measuring run-to-run noise. The report records executable
SHA-256 hashes, CPU topology, worker budgets, every repetition, and completion
checks.

The complete `rpc_bench` suite runs with 2, 4, and 8 total workers, comparing one
shared runtime against separate client/server runtimes with half the workers each.
Separate-process measurements use 2, 4, and 8 workers **per process**, 1/4/16
connections, 64-byte/4-KiB payloads, and up to four independent runtimes per process.
Connections and their caller tasks are assigned to the same runtime; all server
shards retain shared service state. Each shard listens on a separate ephemeral
port, so this measures explicit connection partitioning rather than accept-side
load balancing. Workers are not pinned, so it does not establish NUMA locality.
Three repetitions alternate main/candidate ordering. `--quick` is for harness
validation only and cannot be published as performance evidence.

The metrics workflow's `full_evaluation` manual input runs this comparison on a
reserved cluster host and publishes it to `metrics_history` alongside normal
metrics. These are benchmark options, not additional RPC driver modes.

The full evaluation also runs the smoke harness's realistic service mix, scaling
sweep (10–500 connections), multi-process throughput sweep, CPU-prefix handler,
and dynamic/CPU-affinity pool workloads at 2 and 8 workers per process. Use
`--smoke-only` to collect these as a supplementary run. `--skip-build` on the
smoke harness uses the exact prebuilt sibling executables whose hashes were
recorded, avoiding any rebuild during measurement. Summarize reports with
`python3 rpc/scripts/summarize_runtimes.py path/to/evaluation.json`.
