# xdr_rpc

An async ONC-RPC (RFC 5531) client/server library for Rust, built on tokio and [xdrs](https://github.com/groundhogchain/xdrs).

## Features

- Async TCP-based RPC client and server
- XDR serialization via the `xdrs` crate
- Service registration with program/version dispatching
- **Concurrent dispatch** (opt-in) — spawn-per-CALL with configurable concurrency limit
- **Connection pools** — `FixedSerialPool` (serial per-conn), `DynamicPool` (min-pending), `CpuSetPool` (LLC-affine)
- **Bidirectional RPC** — handlers can call back to the peer via `peer()`
- Panic isolation, load shedding, connection poisoning for clean shutdown
- Max message size limit (16MB default)

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
let client = connect_client(&addr).await?;
let response = client.send_call(ProgVers::new(100000, 1), 0, b"").await?;
```

## Concurrent Dispatch

Enable per-connection concurrent handler execution:

```toml
[dependencies]
rpc_driver = { version = "0.5", features = ["concurrent-dispatch"] }
```

Tune concurrency via `ConnectionConfig::with_max_concurrent_dispatches`.

## Workspace

| Crate | Purpose |
|---|---|
| `rpc_driver` | Core RPC library |
| `rpc-smoke` | Multi-connection smoke test suite |
| `bench-compare` | Benchmark comparison tooling |

## Documentation

```sh
cargo doc --open
```

## Known Issues

- **Multi-part messages unsupported** — fragmented records cause connection shutdown
- **No structured logging** — errors go to stderr (tracking issue: #18)

## License

See [LICENSE](LICENSE).
