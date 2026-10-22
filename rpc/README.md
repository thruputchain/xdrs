# xdr_rpc (rpc_api, rpc_driver)

The ONC-RPC stack lives in this `rpc/` directory of the `xdrs` repository
(it was previously the separate `thruputchain/xdr_rpc` repository). Its
crates are members of the root Cargo workspace.

An async ONC-RPC (RFC 5531) client/server library for Rust, built on tokio and [xdrs](https://github.com/thruputchain/xdrs).

## Features

- Async TCP-based RPC client and server
- XDR serialization via the `xdrs` crate
- Service registration with program/version dispatching
- **Automatic dispatch** — ready handlers finish inline; only suspended handlers spawn tasks
- **Connection pools** — `MultiplexPool` (fixed set, many in flight per connection), `FixedSerialPool` (serial per-conn), `DynamicPool` (min-pending), `CpuSetPool` (LLC-affine)
- **Bidirectional RPC** — handlers can call back to the peer via `peer()`
- Panic isolation and connection poisoning for shutdown
- **Structured tracing** — `tracing` spans and events for logging, OpenTelemetry, and e2e waterfalls
- Max message size limit (16 MiB default)

TCP sockets attempt to enable keepalive with a 60-second idle delay, including
accepted sockets and connections opened by pool recovery. Configuration failures
emit a warning and preserve the connection; some socket options may already have
been applied before the failure.
Probe intervals and retry counts retain OS defaults. Keepalive helps prevent idle
middlebox expiry; it does not impose an RPC deadline or bound recovery from a
network failure. Callers must enforce their own request deadlines.

## Usage

```rust
use rpc_driver::{RpcServer, ServiceMap, connect_client, ProgVers};
use tokio::net::TcpListener;

// Server
let mut services = ServiceMap::new();
services.add_service(Box::new(my_service));
let listener = TcpListener::bind("0.0.0.0:111").await?;
let server = RpcServer::new(listener, services);
let _handle = server.start().await;

// Client
let conn = connect_client(&addr).await?;
let response = conn.send_call(ProgVers::new(100000, 1), 0, conn.call_buf()).await?;
```

## Dispatch and write batching

There is one dispatch path. Every CALL's handler starts on the reader task.
A handler that completes on its first poll replies inline; a suspended handler
moves to its own task, and the reader continues routing CALLs and REPLYs.
Same-connection callbacks through `peer()` work without special configuration.
CPU-heavy handlers should offload work promptly so they do not block the reader.
Handler panics return SYSTEM_ERR without closing the connection.

The writer uses x23-style delayed replies, adapted to Tokio's caller scheduling:

- **Calls** flush immediately unless the reader is actively processing input;
  calls produced by its woken Tokio tasks then share the flush before the
  reader waits/yields, or at 64 records / 64 KiB. This differs from x23's
  `awrite`, which always requests a flush and
  works with direct inline caller resumption. Removing this request coalescing
  significantly reduces pipelined throughput on Tokio.
- **Replies (`dwrite`)** batch while the reader consumes buffered input. They
  flush before the reader waits or yields, when queued output reaches 64 records or 64 KiB,
  or when an outgoing call starts a flush. Replies produced while the reader is waiting go
  out immediately.
  A flush includes earlier calls in wire order. Read-buffer refills do not
  split either kind of batch.
- **Socket backpressure** preserves record order and partial-write offsets.
  One producer drains with `writev`; concurrent producers append. Only a full
  socket needs a writable-readiness task, which exits once output drains.

Unlike x23's refill-triggered reply flush, this driver keeps a burst together
across successful reads. With Tokio's separately scheduled callers, flushing at
every refill fragments replies and regresses shared-runtime throughput. The
record/byte thresholds and bounded reader fairness budget still guarantee flushing
under continuous input.

There is no timer or scheduling-turn delay to accumulate a batch. The reader
also flushes before yielding for fairness under continuous input. Sparse calls
and replies do not wait for another RPC to trigger a flush.

Record allocations are reused through one cache per thread, shared by that
thread's connections without a mutex. Each cache retains at most 256 buffers
and 4 MiB of capacity; buffers larger than 1 MiB are freed. Completed handlers
reuse their input allocation to frame the reply. There is no cache configuration
or connection-pool-specific buffer-sharing policy.

Applications own deadlines. Two optional per-connection settings control call
admission and suspended handlers:

```rust
let config = ConnectionConfig::new(None, services)
    .with_output_admission_threshold_bytes(Some(16 * 1024 * 1024))
    .with_max_suspended_handlers(Some(320));
```

The output admission threshold counts the full allocation capacity of CALL and
REPLY buffers, including active/parked and partially written records. A short
reply reusing a large request buffer is charged for that entire allocation.
Completed records retire at batch boundaries or when the socket blocks.
Outgoing CALLs that would exceed the threshold fail locally with `SystemError`.
At or above the threshold, incoming CALLs receive `SYSTEM_ERR` before their
handler runs. Admission resumes as the backlog drains.

Replies always bypass output admission, including rejection responses and
results of already accepted calls. Reaching the threshold never closes the
connection or discards a queued call or reply. This is not a hard memory bound:
a peer that keeps sending calls without reading responses can still grow the
output queue. The separately bounded thread cache, allocator overhead, and
kernel socket buffers are outside the accounting.

The handler cap allows at most that many suspended tasks plus one inline probe.
Handlers are polled once before admission: a ready handler can still finish
when the cap is full; an excess suspended handler is dropped and receives
`SYSTEM_ERR`. Code before the first suspension may already have run. The reader
continues routing replies, so admitted handlers can complete callbacks.

Both settings default to `None` (unlimited) and can be disabled independently.
`Some(0)` rejects new calls while allowing replies, or allows only ready
handlers, respectively.

### Migration from 0.12

Remove the `concurrent-dispatch` Cargo feature and calls to
`with_inline_ready_dispatch`, `with_max_concurrent_dispatches`,
`with_read_channel_capacity`, `with_write_channel_capacity`,
`with_write_buffer_bytes`, and `with_write_buffer_limit_bytes`.
The corresponding write-buffer constants are removed. The optional settings
above replace the old output policies and pending-task cap; there is no
connection-closing output cap or handler semaphore. Message-size and socket
read-buffer settings remain. Like x23, the message-size limit also rejects
oversized outgoing CALLs before transmission. `rpc_api` and the wire format are unchanged.
`FixedSerialPool` still explicitly serializes client calls per connection;
use a direct connection, `DynamicPool`, or `CpuSetPool` for multiplexed calls.

## Testing without TCP

`rpc_driver::loopback::LoopbackLink` connects two `ServiceMap`s directly,
each the services one end exposes to the other, and hands back each end's
`RpcSender` to the other: no socket, no framing, no xid, no reader task.
It is for unit tests of code that holds an `Arc<dyn RpcSender>`, such as
generated protocol clients, against real handlers.

```rust
use rpc_driver::loopback::LoopbackLink;

// A client that serves nothing, linked to the server's services.
let link = LoopbackLink::connect(ServiceMap::new(), server_services)
    .with_latency(Duration::from_millis(5)); // one-way, on tokio time
let (client, _server) = link.senders();
let client = MyService::new(Arc::new(client));
// ...
link.poison(); // in-flight and later calls fail with SystemError
```

- Latency is applied with `tokio::time::sleep`, once before the handler
  runs and once before the reply returns, so a test started with
  `#[tokio::test(start_paused = true)]` pays no wall-clock time for it and
  can assert on elapsed virtual time. `set_latency` changes it while the
  link is up. `send_fire_and_forget` returns as soon as the call is queued
  and runs the handler one leg later on its own task.
- Handler errors arrive as they would over the wire: wire-level statuses
  pass through, driver-local errors such as `PoolClosed` become
  `SystemError`, and a handler panic becomes `SystemError` without taking
  the link down.
- `peer()` inside a handler is the sender back to the calling end, so an
  end given an empty `ServiceMap` answers callbacks like a client with the
  default `ConnectionConfig`.
- Senders and link clones share one link. `poison()` wakes calls parked on
  it from either end and refuses new ones, like a closed connection; a call
  in flight is dropped where it is, handler included.

The message-size limit, output admission, the suspended-handler cap, and
the pools are not modeled. Use a loopback TCP connection for those.

## Tracing

All diagnostic output uses the [`tracing`](https://docs.rs/tracing) crate. No
subscriber is required; without one, tracing callsites are disabled. Exact
overhead depends on the build, platform, and workload.

### Span hierarchy

```text
rpc_connection{peer=10.0.0.1:4567 role="server"}     INFO  — per TCP connection
  rpc_call{xid=42 prog=100 vers=1 proc=3}            DEBUG — per RPC call
```

### Subscriber setup

Add `tracing-subscriber` to your binary crate and initialize before starting the server:

```rust
use tracing_subscriber::fmt::format::FmtSpan;

tracing_subscriber::fmt()
    .with_max_level(tracing::Level::DEBUG)
    .with_span_events(FmtSpan::CLOSE)
    .init();
```

Successful RPC activity is represented primarily by spans. Normal events still
appear without `FmtSpan::CLOSE`, but span-close records do not. With it, each
span logs its duration on close:

```text
rpc_connection{peer=127.0.0.1:4567 role="server"}:rpc_call{xid=1 prog=100 vers=1 proc=1}: close time.busy=6.2µs time.idle=0.4µs
rpc_connection{peer=127.0.0.1:4567 role="server"}: close time.busy=49ms time.idle=954ms
```

For OpenTelemetry export, use [`tracing-opentelemetry`](https://docs.rs/tracing-opentelemetry) — it captures span start/end natively without `FmtSpan`.

### Performance overhead

Instrumentation cost is workload- and subscriber-dependent. Measure it on the
target system with:

```sh
cargo bench -p thruput-rpc-driver --bench rpc_bench -- --tracing debug
```

See `rpc-smoke/src/server.rs` for a complete reference example.

## Workspace

`rpc_api` and `rpc_driver` are published as `thruput-rpc-api` and
`thruput-rpc-driver`; their library names are unchanged.

| Crate | Purpose |
|---|---|
| `rpc_api` | Stable API boundary: `RpcSender`/`RpcHandler` traits, `CallBuf`, `RpcResponse`, `RpcError`, `ProgramError`, `ProgVers` |
| `rpc_driver` | Core RPC library (transports, pools, server, dispatch); re-exports `rpc_api` |
| `rpc-smoke` | Multi-connection smoke test suite |
| `bench-compare` | Local benchmark comparison tooling |
| `rpc-api-fixture` | Unpublished fixture proving generated-style bindings need only `rpc_api` |

### Versioning and releases

`rpc_api` is the stability contract between `rpc_driver` and generated
protocol bindings (protocol-rs): everything generated code needs lives
there, so implementation-only `rpc_driver` releases don't force protocol
crates to republish. It versions independently of the workspace:

- **`rpc_api` changes should be additive** — new `#[non_exhaustive]`
  `RpcError` variants, new trait methods with default implementations —
  and bump the patch version. A breaking change bumps the minor version
  (0.2.0 changed `RpcSender` to take a `CallBuf`; 0.3.0 replaced
  `CallBuf::encode` with `CallBuf::append_with`) and forces a coordinated
  republish of every generated protocol crate; treat it as a last resort.
  The driver's error encoder maps variants it doesn't know to SYSTEM_ERR,
  so a newer `rpc_api` never changes an older driver's wire behavior.
- **`rpc_api` has no serializer dependency.** Generated clients encode
  their arguments through `CallBuf::append_with` with the `xdrs` they were
  generated against, so an `xdrs` release never forces an `rpc_api`
  release, and never splits the dependency tree between `rpc_api`'s `xdrs`
  and a protocol crate's. Only `rpc_driver` depends on `xdrs`.
- **`rpc_driver` shares the workspace version with the `xdrs` crates**
  (unified at 0.16.0 when the repositories merged) and releases with them,
  depending on the current `rpc_api` minor and re-exporting it at the
  historical paths (the 0.10.2 split was a source-compatible patch).
- **Releases** (`publish.yml`) publish `rpc_api` under its own
  `rpc_api-v<version>` tag first — skipped when unchanged, and failing
  loudly if its source changed without a version bump — then the `xdrs`
  crates and `rpc_driver` under the shared `v<version>` tag. Keep the
  workspace `rpc_api` dependency pin in the root `Cargo.toml` in sync with
  `rpc_api/Cargo.toml`.
- **Migration:** until x23c emits `::rpc_api::` paths and protocol crates
  depend on `rpc_api` directly, a *breaking* driver release still recreates
  the version diamond for pre-split protocol crates — regenerate first.

## Documentation

```sh
cargo doc --open
```

## Known Issues

- **Multi-part messages unsupported** — fragmented records cause connection shutdown

## License

See [LICENSE](../LICENSE).


## Reproducible performance runs

```sh
python3 rpc/scripts/benchmark.py --output target/bench-report
python3 rpc/scripts/benchmark.py --quick --output target/bench-smoke
```

The script builds release binaries with the lockfile, then runs:

- `cargo bench -p thruput-rpc-driver --bench primitives`: x23-style empty function,
  ready future, scalar XDR encode/decode (0, 4, and 400 wire bytes), scheduler
  yield, and buffered TCP record timings. Codec measurements time batches and
  retain seven trials. The scalar loops match x23 wire sizes, not its generated
  struct specialization; current-thread yield is not a cross-thread migration.
- Separate server/client processes using the actual RPC transport. Fixed workers
  reuse connections and finish warmup before a barrier starts measurement. No
  per-request task spawn occurs in the load generator. Each reply is checked.
  The matrix covers 64/4096-byte payloads, 1/32 in-flight calls, and 1/4 connections.
  Each case runs four times in sequence on the driver's single dispatch path, and
  every run records its position so drift across repeats is visible.

`--threads 2` is the default Tokio worker count **per process**. Keep it and the
host constant for comparisons. `--quick` only checks that measurements run; its
short samples are not regression evidence. Full runs use 1000 warmup and 5000
measured calls per worker. Latency runs from invocation to response delivery;
throughput includes client validation and the slowest worker's completion.
This is closed-loop offered load: it does not measure queueing under an externally
fixed request rate, and its tail latency must not be presented as such.

`e2e.json` retains all runs, throughput, p50/p95/p99, payload/concurrency,
completion counts, dispatch mode, source SHA, CPU, Rust version, and worker count.
`primitives.json`, `summary.md`, and diagnostic logs accompany it. Benchmarks do
not execute in this repository's CI. The cluster workflows live in the private
`thruputchain/benchmarks` repository, so no workflow here can reach the hog
cluster's runners. Each push to `main` asks that repository to run the RPC
metrics for it (`.github/workflows/benchmarks.yml`), and maintainers can
dispatch a run there for any commit on an xdrs branch. See that
repository's README for how to request a run. It checks out the selected commit
through `hog_sched` on a dedicated benchmark host, runs these scripts' component
and separate-process matrix, and writes JSON to `groundhogchain/metrics_history`
using hogd's `metrics/<name>/main/<commit-time>_<commit>` layout. The metric
names are `xdr-rpc-e2e` and `xdr-rpc-components`. Failed or partial runs are not
published as successful measurements; diagnostics are printed in the workflow
log. No timing threshold gates PRs.

The manual workflow's `full_evaluation` input additionally compares a baseline
and the candidate with identical harnesses and isolated build directories. The
baseline is where the candidate forked from `main` (on `main` itself, the
previous commit); `--baseline <rev>` picks another. It runs all RPC scenarios,
components, runtime layouts, and smoke workloads with three interleaved
repetitions that rotate the baseline, the candidate, and a candidate control
that reruns the exact candidate executables, so the control's A/A change shows
run-to-run noise. It records executable hashes and host
metadata under `xdr-rpc-runtime-evaluation`. Run it locally with
`python3 rpc/scripts/evaluate_runtimes.py --output target/runtime-evaluation`, then
summarize the raw report with `python3 rpc/scripts/summarize_runtimes.py
target/runtime-evaluation/evaluation.json`. The smoke README describes the runtime
partitioning controls; these belong to the benchmark programs, not the driver.

To measure already-built baseline/candidate binaries with exactly the same
harness, pass `--bin-dir /absolute/path/to/release`. Metadata labels these as
prebuilt and records the path; the SHA identifies the harness checkout, so retain
the binary build SHA alongside the report. Run baseline/candidate alternately on
an otherwise idle host and compare individual repeated runs, not just minima.

These end-to-end results cover Rust client → TCP → Rust RPC server → reply,
including framing, dispatch and scheduling. They are not gateway/transaction
execution benchmarks or a measured claim of parity with C++ x23. A direct C++
comparison also needs the same handler, wire payload, connection count, in-flight
load, CPU placement, and logging settings. Rust and C++ can plausibly reach similar
performance; their scheduler and task/channel allocation paths still differ.
