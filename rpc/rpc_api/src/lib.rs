#![warn(missing_docs)]

//! # thruput_rpc_api
//!
//! Stable API boundary for the Thruput ONC-RPC stack.
//!
//! This crate owns the shared public types that connect three parties:
//!
//! - **`thruput_rpc_driver`** — the transport, pool, and dispatch implementation.
//!   It depends on this crate, re-exports everything here at its historical
//!   paths, and implements [`RpcSender`] for its connections and pools.
//! - **Generated protocol bindings** (protocol-rs clients and dispatchers),
//!   which are generated against this crate only and acquire no transport
//!   dependency.
//! - **Consumers**, which wire generated clients and dispatchers to driver
//!   connections.
//!
//! Because all three share these types through a single, rarely-changing
//! crate, `thruput_rpc_driver` can make breaking implementation-only releases
//! (pools, configuration, transports) without forcing generated protocol
//! crates to republish.
//!
//! ## Stability
//!
//! This crate is intentionally tiny, and its dependencies are limited to
//! `async-trait` and `thiserror`. XDR serialization is the generated
//! bindings' business: they encode through [`CallBuf::append_with`] with
//! the `xdrs` they were generated against, so an `xdrs` release never
//! forces a release here. Changes must be additive wherever
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
//! nothing. [`CallBuf::append_with`] hands the record buffer to a closure
//! that serializes into it in place; generated protocol clients encode
//! their argument tuple that way.
//!
//! ```
//! use thruput_rpc_api::CallBuf;
//!
//! let mut call = CallBuf::new();
//! call.extend_from_slice(&42u32.to_be_bytes());
//! assert_eq!(call.payload(), &[0, 0, 0, 42]);
//! ```
//!
//! ## Chunked messages
//!
//! A message too large for one record goes out as a run of records that
//! may arrive in any order. [`ChunkPosition`] says where a record sits in
//! its run, and a protocol crate implements [`ChunkRecord`] for each of its
//! chunked records; `thruput_rpc_driver::ChunkRunAssembler` reassembles the runs on
//! the server, so handlers push the records they receive as they are.

mod call_buf;
mod chunk;
mod error;
mod future_slot;
mod handler;
mod prog_error;
mod reply_buf;
mod response;
mod sender;

pub use call_buf::CallBuf;
pub use chunk::{ChunkPosition, ChunkRecord};
pub use error::RpcError;
pub use future_slot::{FutureSlot, RpcFuture};
pub use handler::RpcHandler;
pub use prog_error::ProgramError;
pub use reply_buf::ReplyBuf;
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
