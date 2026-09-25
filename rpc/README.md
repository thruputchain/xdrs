# xdr_rpc

An async ONC-RPC (RFC 5531) client/server library for Rust, built on tokio and [xdrs](https://github.com/groundhogchain/xdrs).

## Features

- Async TCP-based RPC client and server
- XDR serialization via the `xdrs` crate
- Service registration with program/version dispatching
- **Automatic dispatch** — ready handlers finish inline; only suspended handlers spawn tasks
- **Connection pools** — `FixedSerialPool` (serial per-conn), `DynamicPool` (min-pending), `CpuSetPool` (LLC-affine)
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
cargo bench -p rpc_driver --bench rpc_bench -- --tracing debug
```

See `rpc-smoke/src/server.rs` for a complete reference example.

## Workspace

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
  (0.2.0 changed `RpcSender` to take a `CallBuf`) and forces a coordinated
  republish of every generated protocol crate; treat it as a last resort.
  The driver's error encoder maps variants it doesn't know to SYSTEM_ERR,
  so a newer `rpc_api` never changes an older driver's wire behavior.
- **`rpc_driver` keeps the workspace version** and its own cadence,
  depending on the current `rpc_api` minor and re-exporting it at the
  historical paths (the 0.10.2 split was a source-compatible patch).
- **Releases** (`publish.yml`) publish `rpc_api` under its own
  `rpc_api-v<version>` tag first — skipped when unchanged, and failing
  loudly if its source changed without a version bump — then `rpc_driver`
  under `v<version>`. Keep the workspace `rpc_api` dependency pin in sync
  with `rpc_api/Cargo.toml`.
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

See [LICENSE](LICENSE).


## Reproducible performance runs

```sh
python3 scripts/benchmark.py --output target/bench-report
python3 scripts/benchmark.py --quick --output target/bench-smoke
```

The script builds release binaries with the lockfile, then runs:

- `cargo bench -p rpc_driver --bench primitives`: x23-style empty function,
  ready future, scalar XDR encode/decode (0, 4, and 400 wire bytes), scheduler
  yield, and buffered TCP record timings. Codec measurements time batches and
  retain seven trials. The scalar loops match x23 wire sizes, not its generated
  struct specialization; current-thread yield is not a cross-thread migration.
- Separate server/client processes using the actual RPC transport. Fixed workers
  reuse connections and finish warmup before a barrier starts measurement. No
  per-request task spawn occurs in the load generator. Each reply is checked.
  The matrix covers 64/4096-byte payloads, 1/32 in-flight calls, and 1/4 connections.
  Concurrent server dispatch runs with the inline probe off/on/on/off (ABBA).

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
not execute in PR CI. Pushes to main after merges trigger the RPC Metrics
workflow; a manual run is also available. It checks out the selected commit
through `hog_sched` on an exclusively reserved host selected by `?`, runs the
component and separate-process matrix, and writes JSON to `groundhogchain/metrics_history` using hogd's
`metrics/<name>/main/<commit-time>_<commit>` layout. The metric names are
`xdr-rpc-e2e` and `xdr-rpc-components`. The workflow forwards repository deploy
keys through the scheduler's SSH agent for checkout and metrics publication.
Failed or partial runs are not published as successful measurements; diagnostics
are printed in the workflow log. No timing threshold gates PRs.

The manual workflow's `full_evaluation` input additionally compares the pinned
main baseline and candidate with identical harnesses and isolated build directories.
It runs all RPC scenarios, components, runtime layouts, and smoke workloads with
three interleaved repetitions, recording executable hashes and host metadata under
`xdr-rpc-runtime-evaluation`. Run it locally with
`python3 scripts/evaluate_runtimes.py --output target/runtime-evaluation`, then
summarize the raw report with `python3 scripts/summarize_runtimes.py
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
