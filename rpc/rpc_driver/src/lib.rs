#![warn(missing_docs)]

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
//! let conn = connect_client(&"127.0.0.1:111".parse()?).await?;
//! let response = conn.send_call(ProgVers::new(100000, 1), 0, conn.call_buf()).await?;
//! // conn automatically shuts down when dropped
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
//! rpc_driver = { version = "0.11", features = ["concurrent-dispatch"] }
//! ```
//!
//! With this feature:
//! - An optional ready-first mode polls handlers once on the dispatch loop;
//!   immediately-ready work replies without a handler task
//! - Multiple handlers execute concurrently per connection (default limit: 64)
//! - Handlers can call back to the peer inline without deadlocking
//! - Handler panics are caught and returned as [`RpcError::SystemError`] —
//!   the connection stays alive for subsequent requests
//! - Load shedding engages when pending tasks exceed 5× the concurrency
//!   limit — excess calls receive `SystemError`
//!
//! Tune the concurrency limit via [`ConnectionConfig::with_max_concurrent_dispatches`]
//! and opt into the ready-first path via
//! [`ConnectionConfig::with_inline_ready_dispatch`]. Work before a handler's
//! first `.await` runs on the dispatch loop and should remain short.
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
//!         let resp = peer.send_call(ProgVers::new(200, 1), 0, peer.call_buf()).await?;
//!         Ok(resp.to_vec())
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
//!             // The read loop can process the callback reply after this
//!             // dispatch returns.
//!             let _ = peer.send_call(ProgVers::new(200, 1), 0, peer.call_buf()).await;
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
//! Without a subscriber, tracing callsites are disabled. Exact overhead depends
//! on the build, platform, and workload.
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
//! - **`rpc_call`** (DEBUG) — created per CALL message. On a concurrent server,
//!   wall duration runs from admission through response enqueue, so semaphore
//!   queueing appears as idle time. On a serial server it covers dispatch and
//!   response enqueue. On the client it measures round-trip time. Common fields:
//!   `xid`, `prog`, `vers`, `proc`; concurrent inbound spans also record
//!   `probed` and, when probed, `ready`.
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
//! // INFO records connection spans; DEBUG also records per-RPC spans.
//! tracing_subscriber::fmt()
//!     .with_max_level(tracing::Level::DEBUG)
//!     .with_span_events(FmtSpan::CLOSE)
//!     .init();
//! ```
//!
//! Successful RPC activity is represented primarily by spans. The `fmt`
//! subscriber still displays normal events without `FmtSpan::CLOSE`, but it
//! does not emit a line when a span ends. `CLOSE` adds that lifecycle record,
//! including `time.busy` (execution time) and `time.idle` (scheduling gaps):
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
//! Instrumentation cost is workload- and subscriber-dependent. Use
//! `cargo bench --bench rpc_bench --features concurrent-dispatch -- --tracing <level>`
//! to measure it on the target system.
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
pub use pool::{CanceledCallPolicy, CpuSetPool, DynamicPool, FixedSerialPool};

/// Low-level TCP record-stream primitives used by the driver.
///
/// This module remains public for integration tests and custom transport
/// experiments, but it is not part of the supported high-level API.
#[doc(hidden)]
pub mod tcp_connection;

// This module is generated, with the following modifications
// over the default x23c generated rust:
// - remove unneeded features (serde, spec)
// - force enable xdrs feature
#[rustfmt::skip]
#[allow(unreachable_pub)]
mod rpc_msg;

mod rpc_error;

// The stable API boundary — sender/handler traits and shared response/error
// types — lives in the independently-versioned `rpc_api` crate so that
// implementation-only driver releases do not force generated protocol
// bindings to republish. Re-exported here at their historical paths for
// source compatibility.
pub use rpc_api::{ProgVers, ProgramError, RpcError};

#[doc(hidden)]
pub mod tasks;
