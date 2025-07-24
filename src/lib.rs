#![no_std]
#![allow(ambiguous_glob_reexports)]

#[cfg(feature = "alloc")]
extern crate alloc;

// Traits for translating Rust values to and from XDR
pub mod discrim;
pub mod read_xdr;
pub mod write_xdr;

// The abstract interface for serialization formats
pub mod serialize;

// CXS serialization
pub mod serialize_cxs;

// TXS serialization
#[cfg(feature = "txs")]
pub mod serialize_txs;

pub mod mu; // used by integration tests
pub(crate) mod valid_u32;

pub mod prelude {
    pub use crate::read_xdr::*;
    pub use crate::write_xdr::*;

    pub use crate::discrim::*;

    pub use crate::serialize::reader::Error as ReadError;
    pub use crate::serialize::reader::Result as ReadResult;
    pub use crate::serialize::reader::*;

    pub use crate::serialize::writer::Error as WriteError;
    pub use crate::serialize::writer::Result as WriteResult;
    pub use crate::serialize::writer::*;

    #[cfg(feature = "derive")]
    pub use ::xdr_derive::*;
}

pub use prelude::*;
