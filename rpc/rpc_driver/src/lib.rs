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
//! ## Dispatch and automatic write batching
//!
//! Each handler is polled inline. Ready handlers reply without spawning a
//! task; suspended handlers move to a task while the reader keeps serving
//! CALLs and REPLYs. There is one dispatch path, with no feature flags.
//! Handler panics return [`RpcError::SystemError`]. CPU-heavy handlers should
//! offload their work so their first poll does not block the connection.
//!
//! Calls flush immediately unless the reader is actively processing input;
//! then they share its flush before waiting/yielding or at the record/byte thresholds.
//! Unlike x23's direct caller resumption,
//! Tokio schedules woken callers separately, so request coalescing matters.
//! Replies accumulate until an outgoing flush, 64 records or 64 KiB, or
//! the reader waits/yields. A flush includes preceding calls in wire order.
//! Successful read-buffer refills do not split output batches.
//! Replies produced while the reader is waiting flush immediately. Only a
//! full socket needs a background writable-readiness task. Applications own
//! admission control; the transport neither queues handlers behind permits
//! nor rejects calls based on its output backlog.
//!
//! ## Bidirectional RPC
//!
//! Inside a handler, use [`peer()`] to get the per-connection
//! [`RpcSender`]. This is stored in task-local context and set
//! automatically by the dispatch loop.
//!
//! Handlers can call back to the peer inline:
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
//!         let resp = peer.send_call(ProgVers::new(200, 1), 0, peer.call_buf()).await?;
//!         Ok(resp.to_vec())
//!     }
//! # }
//! ```
//!
//! [`try_peer()`] returns `Option` and is safe to call outside a handler.
//!
//! ## Testing without TCP
//!
//! [`loopback::LoopbackLink`] connects two [`ServiceMap`]s directly and
//! hands back each end's [`RpcSender`] to the other, with optional one-way
//! latency on tokio time, so unit tests of code that holds an
//! `Arc<dyn RpcSender>` run against real handlers with no socket and, under
//! a paused clock, no waiting.
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
//! rpc_connection{peer=192.0.2.1:4567 role="server"}     [INFO]  — per TCP connection
//!   rpc_call{xid=42 prog=100 vers=1 proc=3}            [DEBUG] — per RPC call
//! ```
//!
//! - **`rpc_connection`** (INFO) — created on accept (server) or connect (client).
//!   Lives for the connection's lifetime. Fields: `peer` (socket address), `role`
//!   (`"server"` or `"client"`).
//!
//! - **`rpc_call`** (DEBUG) — created per CALL message. On the server, wall
//!   duration covers dispatch through response enqueue. On the client it
//!   measures round-trip time. Common fields: `xid`, `prog`, `vers`, `proc`.
//!   Inbound spans record `ready`: true for inline completion, false when
//!   the handler suspends and moves to a task.
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
//! `cargo bench --bench rpc_bench -- --tracing <level>`
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

pub mod chunk;
pub use chunk::*;

mod config;
pub use config::ConnectionConfig;

mod server;
pub use server::RpcServer;

mod client;
pub use client::{connect_client, connect_client_with_config};

pub mod pool;
pub use pool::{CanceledCallPolicy, CpuSetPool, DynamicPool, FixedSerialPool, MultiplexPool};

pub mod loopback;

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
