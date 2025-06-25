#![no_std]
#![feature(maybe_uninit_array_assume_init)]
#![feature(maybe_uninit_write_slice)]
#![allow(ambiguous_glob_reexports)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod bytes;

// Traits for translating Rust values to and from XDR
pub mod discrim;
pub mod read_xdr;
pub mod write_xdr;

// The abstract interface for serialization formats
pub mod xdr;

// An implementation of CXS serialization
pub mod xdr_cxs;

pub mod prelude {
    pub use crate::bytes::*;
    pub use crate::discrim::*;
    pub use crate::read_xdr::*;
    pub use crate::write_xdr::*;
    pub use crate::xdr::reader::Error as ReadError;
    pub use crate::xdr::reader::Result as ReadResult;
    pub use crate::xdr::reader::*;
    pub use crate::xdr::writer::Error as WriteError;
    pub use crate::xdr::writer::Result as WriteResult;
    pub use crate::xdr::writer::*;

    #[cfg(feature = "derive")]
    pub use ::xdr_derive::*;
}
pub use prelude::*;
