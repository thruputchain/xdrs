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
//! ## Bidirectional RPC
//!
//! Inside a handler, use [`peer()`] to get the per-connection
//! [`RpcSender`]. This is stored in task-local context and set
//! automatically by the dispatch loop.
//!
//! **Important:** The read loop awaits `dispatch` inline, so calling back
//! to the peer directly will deadlock. Spawn a separate task instead:
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

// Lints on the generate rpc_msg.rs file
#![allow(clippy::upper_case_acronyms)]
#![allow(clippy::match_single_binding)]

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
pub use pool::{DynamicPool, FixedPool};

pub mod tcp_connection;

// This module is generated, with the following modifications
// over the default x23c generated rust:
// - remove unneeded features (serde, spec)
// - force enable xdrs feature
#[rustfmt::skip]
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
