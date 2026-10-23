# rpc-smoke

Multi-connection smoke and load test for `rpc_driver`. The harness starts one
server and many client processes; each client drives a handler profile (echo,
sleep, mutex contention, CPU spin) through one of the connection pools, so a
run exercises concurrent dispatch under a mixed load.

## Quick Start

```sh
cargo build --release -p rpc-smoke

# Every built-in service profile at once against one server
cargo run --release -p rpc-smoke --bin rpc-smoke -- realistic --duration 10

# Multi-process throughput test
cargo run --release -p rpc-smoke --bin rpc-smoke -- throughput --clients 1,4,8,16 --duration 10
```

## Binaries

| Binary | Purpose |
|---|---|
| `rpc-smoke-server` | RPC server that simulates each request's handler profile (echo, sleep, mutex, CPU spin) |
| `rpc-smoke-client` | Drives one profile's RPCs through a `FixedSerialPool`, `DynamicPool`, `CpuSetPool`, or `MultiplexPool`; outputs JSON metrics |
| `rpc-smoke` | Harness that orchestrates a server and N client processes |
| `rpc-bench-client` | Fixed-worker end-to-end client used by `rpc/scripts/benchmark.py` and `evaluate_runtimes.py` |

## Profiles

A profile fixes a client's default connection count, its request payload
size, and the handler work the server simulates for each request. The service
names describe the load shape a profile stands for in a multi-process
deployment; nothing else depends on them.

| Profile | Connections | Payload | Server handler |
|---|---|---|---|
| `coordinator` | 20 | 1500 B | 1 ms sleep, then 1 ms holding a shared mutex |
| `gateway` | 15 | 800 B | 1 ms sleep, then 1 ms holding a shared mutex |
| `executor` | 10 | 200 B | 500 µs CPU spin (on the blocking pool) |
| `db-shard` | 1 | 1000 B | 2 ms sleep |
| `auth` | 1 | 500 B | 200 µs CPU spin (on the blocking pool) |
| `echo` | 1 | 64 B | Echo |
| `cpu-prefix-ready` | 1 | 64 B | 25 µs CPU spin inline, completing without suspending |

An unrecognized profile name runs as a 64-byte echo profile on one connection.

## Client Options

| Flag | Default | Description |
|---|---|---|
| `--addr` | (required) | Server address (host:port) |
| `--profile` | (required) | Profile name (see [Profiles](#profiles)) |
| `--connections` | profile default | Connection count: the pool size, or `DynamicPool`'s maximum; `CpuSetPool` opens one per LLC instead |
| `--duration` | 5 | Test duration in seconds |
| `--max-inflight` | 100 | Max concurrent in-flight RPCs |
| `--pool` | fixed | Pool type: `fixed`, `dynamic`, `cpu`, or `multiplex` |
| `--tracing` | (none) | Enable tracing subscriber at given level (info, debug) |

The server always runs ready handlers inline, spawns suspended handlers, and
batches replies automatically at read-buffer boundaries. No dispatch flags are
needed. Configure application load with the client's `--max-inflight` option.

## Pool Types

| Pool | Per-conn concurrency | Routing | Comparable x23 client |
|---|---|---|---|
| `fixed` | 1 in-flight (serial) | FIFO via channel | Sync RPC (`SRpcMulticlient`) |
| `dynamic` | Many in-flight | Min-pending load balance | Async RPC (`RpcMulticlient`) |
| `cpu` | Many in-flight | LLC-affine (sched_getcpu) | `RpcMulticlient` / `PerCpuSet` |
| `multiplex` | Many in-flight | Least in flight, fixed set | Async RPC over N connections |

## Harness Options

| Flag | Default | Description |
|---|---|---|
| `--duration` | 5 | Test duration per client (seconds) |
| `--server-addr` | (spawn local) | Remote server address (host:port) |
| `--pool` | (none = fixed) | Pool type for all clients: `fixed`, `dynamic`, `cpu`, or `multiplex` |
| `--tracing` | (none) | Enable tracing subscriber on server and client (info, debug) |
| `--json` | false | Emit machine-readable metrics, including dispatch mode and completion counts |
| `--skip-build` | false | Use the already-built sibling binaries (for controlled comparisons) |

## Harness Scenarios

| Scenario | What it tests | Options (default) |
|---|---|---|
| `realistic` | Every service profile at once: 1 coordinator, the gateways, 4 executors, 4 db-shards, and 1 auth client (13 client processes, 110 connections at the defaults) | `--gateways` (3) |
| `scaling` | Single-client echo with increasing connection count | `--counts` (10,50,100,200,500) |
| `throughput` | Multi-process echo to find the server-side scaling ceiling | `--clients` (1,2,4,8), `--connections-per-client` (10), `--max-inflight` (1000) |
| `single` | One named profile | `--connections`, `--max-inflight` (profile and client defaults) |

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

for POOL in fixed dynamic cpu multiplex; do
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

This produces a 3×4 matrix (3 scenarios × 4 pools) that directly answers:

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
for POOL in fixed dynamic cpu multiplex; do
  rpc-smoke single --server-addr $SERVER --pool $POOL executor --duration $DUR
  rpc-smoke single --server-addr $SERVER --pool $POOL coordinator --duration $DUR
done
```

#### Long soak — leak and stability

Any memory leaks, latency drift, or stalls over minutes?

```sh
# Realistic workload, 5 minutes per pool:
for POOL in fixed dynamic cpu multiplex; do
  rpc-smoke realistic --server-addr $SERVER --pool $POOL --duration 300
done

# Throughput saturation, 5 minutes:
rpc-smoke throughput --server-addr $SERVER --pool dynamic \
    --clients 16 --connections-per-client 10 \
    --max-inflight 2000 --duration 300
```

### How the pool types compare to x23's RPC clients

x23, the C++ ONC-RPC stack whose connection patterns these pools follow, has
two client modes:

- **Sync RPC** (`SRpcMulticlient`): 1 in-flight per connection; throughput
  comes from opening more connections. **FixedSerialPool** works the same way:
  serial dispatch per connection, throughput scales with connection count.

- **Async RPC** (`RpcMulticlient` + `PerCpuSet`): many in-flight per
  connection, one connection per LLC, XID-multiplexed. **DynamicPool**,
  **CpuSetPool**, and **MultiplexPool** work this way: many calls in flight per
  connection, so throughput comes from multiplexing rather than connection
  count.

x23 reaches millions of RPCs/sec with a thread-per-core architecture: each core
runs its own epoll+coroutine loop with no cross-thread synchronization. This
server uses tokio's work-stealing scheduler instead, which is simpler but adds
overhead from task migration and shared queues. **Expect the server to be the
bottleneck**: the multi-process throughput test is the key measurement, since
it isolates server-side scaling by driving the server from independent client
processes.

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
reserved cluster host and records it alongside the normal metrics. These are
benchmark options, not additional RPC driver modes.

The full evaluation also runs the smoke harness's realistic service mix, scaling
sweep (10–500 connections), multi-process throughput sweep, CPU-prefix handler,
and dynamic/CPU-affinity pool workloads at 2 and 8 workers per process. Use
`--smoke-only` to collect these as a supplementary run. `--skip-build` on the
smoke harness uses the exact prebuilt sibling executables whose hashes were
recorded, avoiding any rebuild during measurement. Summarize reports with
`python3 rpc/scripts/summarize_runtimes.py path/to/evaluation.json`.
