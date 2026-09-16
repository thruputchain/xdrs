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
//! `async-trait`, `thiserror`, and the `xdrs` serializer generated bindings
//! already use. Changes must be additive wherever
//! possible: new [`RpcError`] variants are non-breaking thanks to
//! `#[non_exhaustive]`, and a new trait method ships with a default
//! implementation when a correct one exists for every sender. A breaking
//! change here forces a coordinated republish of every generated protocol
//! crate — treat it as a last resort.
//!
//! ## Sending a call
//!
//! Callers take a [`CallBuf`] from the sender with [`RpcSender::call_buf`],
//! encode the arguments into it, and pass it to [`RpcSender::send_call`].
//! The buffer keeps the wire prefix reserved ahead of the payload so the
//! driver frames the call in place, and driver senders recycle spent record
//! buffers through `call_buf`, so a steady stream of calls allocates
//! nothing. [`CallBuf::encode`] serializes XDR values into it directly;
//! generated protocol clients use that path.
//!
//! ```
//! use rpc_api::CallBuf;
//!
//! let mut call = CallBuf::new();
//! call.extend_from_slice(&42u32.to_be_bytes());
//! assert_eq!(call.payload(), &[0, 0, 0, 42]);
//! ```

mod call_buf;
mod error;
mod handler;
mod prog_error;
mod response;
mod sender;

pub use call_buf::CallBuf;
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
