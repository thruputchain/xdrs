# xdr_rpc

An async ONC-RPC (RFC 5531) client/server library for Rust, built on tokio and [xdrs](https://github.com/groundhogchain/xdrs).

## Features

- Async TCP-based RPC client and server
- XDR serialization via the `xdrs` crate
- Service registration with program/version dispatching
- Connection poisoning for clean shutdown
- Max message size limit (16MB default)

## Usage

```rust
use rpc_driver::{RpcServer, connect_client, ProgVers};

// Server
let server = RpcServer::new();
server.register(ProgVers::new(100000, 1), my_service);
server.listen("0.0.0.0:111").await?;

// Client
let client = connect_client(&addr).await?;
```

## Documentation

```sh
cargo doc --open
```

## Known Issues

- **Multi-part messages unsupported** - fragmented records cause connection shutdown
- **No structured logging** - errors go to stderr

## License

See [LICENSE](LICENSE).
