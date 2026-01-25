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
use rpc_driver::{RpcServer, ServiceMap, connect_client, ProgVers};
use tokio::net::TcpListener;

// Server
let mut service_map = ServiceMap::new();
service_map.add_service(Box::new(my_service));
let listener = TcpListener::bind("0.0.0.0:111").await?;
let server = RpcServer::new(service_map, listener);
server.run().await?;

// Client
let client = connect_client(&addr).await?;
let response = client.send_call(ProgVers::new(100000, 1), 0, b"").await?;
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
