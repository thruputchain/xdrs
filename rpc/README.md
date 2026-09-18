# xdr_rpc

An async ONC-RPC (RFC 5531) client/server library for Rust, built on tokio and [xdrs](https://github.com/groundhogchain/xdrs).

## Features

- Async TCP-based RPC client and server
- XDR serialization via the `xdrs` crate
- Service registration with program/version dispatching
- **Concurrent dispatch** (opt-in) — spawn-per-CALL with a configurable
  concurrency limit and an optional inline fast path for handlers that finish
  on their first poll
- **Connection pools** — `FixedSerialPool` (serial per-conn), `DynamicPool` (min-pending), `CpuSetPool` (LLC-affine)
- **Bidirectional RPC** — handlers can call back to the peer via `peer()`
- Panic isolation and load shedding with `concurrent-dispatch`; connection
  poisoning for shutdown
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

## Concurrent Dispatch

Enable per-connection concurrent handler execution:

```toml
[dependencies]
rpc_driver = { version = "0.11", features = ["concurrent-dispatch"] }
```

Tune concurrency via `ConnectionConfig::with_max_concurrent_dispatches`.

Enable the ready-first path with
`ConnectionConfig::with_inline_ready_dispatch(true)`. Each CALL's handler is
then polled once on the dispatch loop. Handlers that finish on that first poll —
null procs, cache hits, anything that enqueues work and returns — are answered
without a handler task. Handlers that suspend are moved into a task, keeping the
progress their first poll made. This is x23's `await_ready()` shape: create
asynchronous handler work only when the service actually suspends.

The permit is reserved *before* that first poll, so `max_concurrent_dispatches`
keeps bounding handler execution even for work a handler starts before it
suspends. While a connection is saturated the fast path stands down and
does not barge ahead of work already waiting for a permit.

Ready responses use a nonblocking enqueue on the connection's writer so output
backpressure cannot stall dispatch ahead of a bidirectional REPLY. If the
writer already holds `write_channel_capacity` records, response sends are
deferred up to a bounded backlog of 5x that capacity; exceeding that limit
fails the connection closed rather than allowing unbounded memory growth while
the peer is not reading.

Each concurrent inbound `rpc_call` span records `probed`; probed calls also
record whether they were `ready`. This distinguishes handlers that suspended
from calls that were not probed because the connection was saturated or the
fast path was disabled.
`with_inline_ready_dispatch(false)` provides an unconditional-spawn control,
which is mainly useful for A/B measurement. It still uses the new common
boxed-handler construction, so compare separate builds for an exact before/after
measurement.

One thing to know: every probed handler starts on the dispatch loop, so
CPU-heavy work before its first `.await` delays dispatching the next record on
that connection even if the handler later suspends. Handlers that block should
suspend promptly — via `spawn_blocking` or a worker queue — rather than compute
inline.

Without the feature, dispatch is serial: each connection awaits its handler
before reading the next record. Serial dispatch has no per-CALL task spawn or
record copy, but one slow handler blocks that connection, and a handler that
calls back to the peer via `peer()` on the same connection will deadlock.

To benchmark the two paths, always pass `-p rpc_driver`:

```sh
cargo bench -p rpc_driver --bench rpc_bench -- --quick                                  # serial
cargo bench -p rpc_driver --bench rpc_bench --features concurrent-dispatch -- --quick   # concurrent
```

Omitting `-p rpc_driver` selects the whole workspace, and `rpc-smoke` enables
`concurrent-dispatch` — cargo's feature unification then turns the feature on
for every member, so a "serial" run silently measures the concurrent path.

## Write path

There is no writer task. A record is written to the socket by the task that
produces it, with a single non-blocking `writev`, and a flusher task exists
only while the socket is full. This is x23's `awrite`/`dwrite` shape, and it
matters on a multi-threaded runtime: every task hand-off can unpark an idle
worker, and those wakeups — not serialization — dominated the per-call cost.

Writes still coalesce. Each connection's read loop tells the writer when it is
working through buffered records and when it is about to wait for the socket.
Replies queue until the read loop goes idle, so a burst of pipelined calls is
answered with one `writev`; calls made while the read loop is busy ride along
in that same flush, and calls made at any other time go out immediately. A
queue over 64 records or 64 KiB flushes regardless, as does a queue that has
reached `write_channel_capacity`. Calls are never held while a serial handler
is running, so a handler's calls to its peer cannot deadlock, and a serial
read loop flushes before it blocks on a handler so replies already produced
are not delayed by it.

`ConnectionConfig::with_write_channel_capacity` bounds the queue: senders wait
once that many records are pending.

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
cargo bench -p rpc_driver --bench rpc_bench --features concurrent-dispatch -- --tracing debug
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
workflow; a manual rerun on main is also available. It dispatches the exact
merged source through `hog_sched` to an exclusively reserved host selected by `?`, runs the
full matrix, and writes JSON to `groundhogchain/metrics_history` using hogd's
`metrics/<name>/main/<commit-time>_<commit>` layout. The metric names are
`xdr-rpc-e2e` and `xdr-rpc-components`. The source travels as a git archive;
`--source-sha` preserves its identity without copying checkout credentials.
Publication uses the selected cluster host's metrics-history credentials. Failed or partial
runs are not published as successful measurements; their logs remain in the job
artifacts. Results are also shown in the job summary and retained as artifacts
for 90 days. No timing threshold gates PRs.

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
