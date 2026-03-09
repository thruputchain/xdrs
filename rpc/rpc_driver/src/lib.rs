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

// Lints on the generate rpc_msg.rs file
#![allow(clippy::upper_case_acronyms)]
#![allow(clippy::match_single_binding)]

pub mod client_state;
pub use client_state::*;

pub mod server_state;
pub use server_state::*;

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
