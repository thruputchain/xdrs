# rpc

An async ONC-RPC (RFC 5531) client and server API for Rust.

| Crate | Purpose |
|---|---|
| `thruput-rpc-api` | Stable traits and types for generated bindings |
| `thruput-rpc-driver` | Transports, pools, server, dispatch; re-exports `thruput-rpc-api` |
| `rpc-api-fixture` | Proves generated bindings need only `thruput-rpc-api` |

`thruput-rpc-api` versions independently: keep changes additive (patch bump); a
breaking change forces every generated protocol crate to republish. It has no
`thruput-xdrs` dependency. `thruput-rpc-driver` is versioned and released with
`thruput-xdrs`.

## Features

- Async TCP client and server with program/version dispatch
- Ready handlers reply inline; only suspended handlers spawn tasks
- Bidirectional RPC: handlers can call back to the peer via `peer()`
- Pools: `MultiplexPool`, `FixedSerialPool`, `DynamicPool`, `CpuSetPool`
- Handler panics return `SYSTEM_ERR` without closing the connection
- `tracing` spans for logging and OpenTelemetry
- 16 MiB default message-size limit; TCP keepalive (60 s idle)

Callers own request deadlines; keepalive does not bound RPC latency.

## Usage

```rust
use rpc_driver::{RpcServer, ServiceMap, connect_client, ProgVers};
use tokio::net::TcpListener;

// Server
let mut services = ServiceMap::new();
services.add_service(Box::new(my_service));
let listener = TcpListener::bind("0.0.0.0:111").await?;
let _handle = RpcServer::new(listener, services).start().await;

// Client
let conn = connect_client(&addr).await?;
let response = conn.send_call(ProgVers::new(100000, 1), 0, conn.call_buf()).await?;
```

## Connection limits

```rust
let config = ConnectionConfig::new(None, services)
    .with_output_admission_threshold_bytes(Some(16 * 1024 * 1024))
    .with_max_suspended_handlers(Some(320));
```

- **Output admission threshold** (bytes of queued output): an outgoing call
  that would exceed it fails locally with `SystemError`; once the backlog is
  at or above it, incoming calls get `SYSTEM_ERR`. Replies always go through.
  Not a hard memory bound.
- **Max suspended handlers**: excess suspended handlers are dropped with
  `SYSTEM_ERR`; ready handlers still complete.

Both default to `None` (unlimited).

## Testing without TCP

`rpc_driver::loopback::LoopbackLink` connects two `ServiceMap`s in-process
for unit tests of code that holds an `Arc<dyn RpcSender>`:

```rust
use rpc_driver::loopback::LoopbackLink;

let link = LoopbackLink::connect(ServiceMap::new(), server_services)
    .with_latency(Duration::from_millis(5)); // one-way, tokio time
let (client, _server) = link.senders();
let client = MyService::new(Arc::new(client));
link.poison(); // in-flight and later calls fail with SystemError
```

Latency uses tokio time, so `start_paused = true` tests run instantly.
Message-size limits, admission, and pools are not modeled.

## Tracing

Spans: `rpc_connection{peer, role}` (INFO) → `rpc_call{xid, prog, vers, proc}`
(DEBUG). Use `FmtSpan::CLOSE` with `tracing-subscriber` to log durations, or
`tracing-opentelemetry` for export.

## Known issues

- Multi-part (fragmented) records cause connection shutdown.

## License

See [LICENSE](../LICENSE).
