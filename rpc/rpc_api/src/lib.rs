#![warn(missing_docs)]

//! # rpc_api
//!
//! Stable API boundary for the Groundhog ONC-RPC stack.
//!
//! This crate owns the shared public types that connect three parties:
//!
//! - **`rpc_driver`** — the transport, pool, and dispatch implementation.
//!   It depends on this crate, re-exports everything here at its historical
//!   paths, and implements [`RpcSender`] for its connections and pools.
//! - **Generated protocol bindings** (protocol-rs clients and dispatchers),
//!   which are generated against this crate only and acquire no transport
//!   dependency.
//! - **Consumers**, which wire generated clients and dispatchers to driver
//!   connections.
//!
//! Because all three share these types through a single, rarely-changing
//! crate, `rpc_driver` can make breaking implementation-only releases
//! (pools, configuration, transports) without forcing generated protocol
//! crates to republish.
//!
//! ## Stability
//!
//! This crate is intentionally tiny, and its dependencies are limited to
//! `async-trait` and `thiserror`. Changes must be additive wherever
//! possible: new [`RpcError`] variants are non-breaking thanks to
//! `#[non_exhaustive]`, and new trait methods must ship with default
//! implementations. A breaking change here forces a coordinated republish
//! of every generated protocol crate — treat it as a last resort.

mod error;
mod handler;
mod prog_error;
mod response;
mod sender;

pub use error::RpcError;
pub use handler::RpcHandler;
pub use prog_error::ProgramError;
pub use response::RpcResponse;
pub use sender::RpcSender;

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
