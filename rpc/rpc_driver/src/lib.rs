//! # rpc_driver
//!
//! Async ONC-RPC (RFC 5531) client/server library built on tokio.
//!
//! ## Quick Start
//!
//! ```no_run
//! use rpc_driver::{connect_client, ProgVers};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = connect_client(&"127.0.0.1:111".parse()?).await?;
//! let response = client.send_call(ProgVers::new(100000, 1), 0, b"").await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Implementing a Service
//!
//! Implement [`RpcHandler`] to handle incoming RPC calls. Register handlers
//! with a [`ServiceMap`] and pass it to [`RpcServer`]:
//!
//! ```no_run
//! use rpc_driver::{ProgVers, RpcError, RpcServer, ServiceMap, handler::RpcHandler};
//! use async_trait::async_trait;
//!
//! struct MyService;
//!
//! #[async_trait]
//! impl RpcHandler for MyService {
//!     async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
//!         match proc {
//!             0 => Ok(vec![]),           // null
//!             1 => Ok(data.to_vec()),    // echo
//!             _ => Err(RpcError::InvalidProcedure),
//!         }
//!     }
//!     fn prog_vers(&self) -> ProgVers { ProgVers::new(100, 1) }
//! }
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut services = ServiceMap::new();
//! services.add_service(Box::new(MyService));
//! let listener = tokio::net::TcpListener::bind("0.0.0.0:111").await?;
//! let server = RpcServer::new(listener, services);
//! server.start().await;
//! # Ok(())
//! # }
//! ```
//!
//! ## Concurrent Dispatch
//!
//! By default, each connection processes one RPC at a time (serial dispatch).
//! Enable the `concurrent-dispatch` feature to run handlers concurrently on
//! a single connection:
//!
//! ```toml
//! [dependencies]
//! rpc_driver = { version = "0.5", features = ["concurrent-dispatch"] }
//! ```
//!
//! With this feature:
//! - Multiple handlers execute concurrently per connection (default limit: 64)
//! - Handlers can call back to the peer inline without deadlocking
//! - Handler panics are caught and returned as [`RpcError::SystemError`] —
//!   the connection stays alive for subsequent requests
//! - Load shedding engages when pending tasks exceed 5× the concurrency
//!   limit — excess calls receive `SystemError`
//!
//! Tune the concurrency limit via [`ConnectionConfig::with_max_concurrent_dispatches`].
//!
//! ## Bidirectional RPC
//!
//! Inside a handler, use [`peer()`] to get the per-connection
//! [`RpcSender`]. This is stored in task-local context and set
//! automatically by the dispatch loop.
//!
//! With the `concurrent-dispatch` feature enabled, handlers run
//! concurrently and can call back to the peer inline:
//!
//! ```no_run
//! # use rpc_driver::{ProgVers, RpcError, handler::RpcHandler};
//! # use async_trait::async_trait;
//! # struct MyService;
//! # #[async_trait]
//! # impl RpcHandler for MyService {
//! #   fn prog_vers(&self) -> ProgVers { ProgVers::new(100, 1) }
//!     async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
//!         let peer = rpc_driver::peer();
//!         // With concurrent-dispatch: works inline
//!         let resp = peer.send_call(ProgVers::new(200, 1), 0, b"").await?;
//!         Ok(resp)
//!     }
//! # }
//! ```
//!
//! Without the feature (default serial dispatch), calling back inline
//! deadlocks. Spawn a separate task instead:
//!
//! ```no_run
//! # use rpc_driver::{ProgVers, RpcError, handler::RpcHandler};
//! # use async_trait::async_trait;
//! # struct MyService;
//! # #[async_trait]
//! # impl RpcHandler for MyService {
//! #   fn prog_vers(&self) -> ProgVers { ProgVers::new(100, 1) }
//!     async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
//!         let peer = rpc_driver::peer();
//!         tokio::spawn(async move {
//!             // safe — runs after dispatch returns
//!             let _ = peer.send_call(ProgVers::new(200, 1), 0, b"").await;
//!         });
//!         Ok(vec![])
//!     }
//! # }
//! ```
//!
//! [`try_peer()`] returns `Option` and is safe to call outside a handler.
//!
//! ## Tracing
//!
//! All diagnostic output uses the [`tracing`](https://docs.rs/tracing) crate.
//! `rpc_driver` emits spans and events — your binary crate chooses the
//! subscriber (console logging, OpenTelemetry, JSON, etc.).
//!
//! **Without a subscriber**, tracing is a no-op (~1-2ns per callsite).
//!
//! ### Span Hierarchy
//!
//! ```text
//! rpc_connection{peer=10.0.0.1:4567 role="server"}     [INFO]  — per TCP connection
//!   rpc_call{xid=42 prog=100 vers=1 proc=3}            [DEBUG] — per RPC call
//! ```
//!
//! - **`rpc_connection`** (INFO) — created on accept (server) or connect (client).
//!   Lives for the connection's lifetime. Fields: `peer` (socket address), `role`
//!   (`"server"` or `"client"`).
//!
//! - **`rpc_call`** (DEBUG) — created per CALL message. On the server, the span
//!   measures handler dispatch time. On the client, it measures round-trip time
//!   (send to receive). Fields: `xid`, `prog`, `vers`, `proc`.
//!
//! ### Subscriber Setup
//!
//! Add `tracing-subscriber` to your binary crate:
//!
//! ```toml
//! [dependencies]
//! tracing = "0.1"
//! tracing-subscriber = { version = "0.3", features = ["fmt"] }
//! ```
//!
//! Then initialize before starting the server or connecting clients:
//!
//! ```no_run
//! use tracing_subscriber::fmt::format::FmtSpan;
//!
//! // INFO: connection lifecycle only (< 1% overhead)
//! // DEBUG: per-RPC waterfall tracing (~5% overhead in realistic workloads)
//! tracing_subscriber::fmt()
//!     .with_max_level(tracing::Level::DEBUG)
//!     .with_span_events(FmtSpan::CLOSE)
//!     .init();
//! ```
//!
//! **`FmtSpan::CLOSE` is important** — without it, the `fmt` subscriber only
//! shows error/warn events. `CLOSE` adds a line when each span ends, showing
//! `time.busy` (execution time) and `time.idle` (scheduling gaps):
//!
//! ```text
//! rpc_connection{peer=127.0.0.1:4567 role="server"}:rpc_call{xid=1 prog=100 vers=1 proc=1}: close time.busy=6.2µs time.idle=0.4µs
//! ```
//!
//! For OpenTelemetry export, use `tracing-opentelemetry` instead — it captures
//! span start/end natively and does not need `FmtSpan`.
//!
//! ### Performance
//!
//! | Subscriber | Per-RPC overhead | Use case |
//! |---|---|---|
//! | None | ~1-2ns (atomic check) | Production default |
//! | INFO | < 1% | Connection monitoring |
//! | DEBUG | ~5% (realistic workloads) | Performance waterfall |
//!
//! See `rpc-smoke/src/server.rs` for a complete reference example of
//! subscriber setup in a production-like server.

// Lint configuration is in workspace Cargo.toml [workspace.lints].
// upper_case_acronyms and match_single_binding are allowed workspace-wide
// for the generated rpc_msg.rs file.

pub mod connection;
pub use connection::*;

mod context;
pub use context::{peer, try_peer};

pub mod handler;
pub use handler::*;

mod config;
pub use config::ConnectionConfig;

mod server;
pub use server::RpcServer;

mod client;
pub use client::{connect_client, connect_client_with_config};

pub mod pool;
pub use pool::{CpuSetPool, DynamicPool, FixedSerialPool};

pub mod tcp_connection;

// This module is generated, with the following modifications
// over the default x23c generated rust:
// - remove unneeded features (serde, spec)
// - force enable xdrs feature
#[rustfmt::skip]
#[allow(unreachable_pub)]
mod rpc_msg;

mod rpc_error;
pub use rpc_error::RpcError;

mod prog_error;
pub use prog_error::ProgramError;

#[doc(hidden)]
pub mod tasks;

/// RPC program and version identifier.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProgVers {
    prog: u32,
    vers: u32,
}

impl ProgVers {
    /// Create a new program/version pair.
    pub const fn new(prog: u32, vers: u32) -> Self {
        Self { prog, vers }
    }

    /// Returns the program number.
    pub fn prog(&self) -> u32 {
        self.prog
    }

    /// Returns the version number.
    pub fn vers(&self) -> u32 {
        self.vers
    }
}
