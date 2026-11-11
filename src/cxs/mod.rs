#![doc = include_str!("../../CXS.md")]

mod utils;
pub use utils::*;

mod limits;

pub mod buf_reader;
pub mod into_buf_writer;

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
pub mod buf_writer;

mod put;

// The const writer and worst-case size bounds are part of the stable trait
// layer (derived types name them in their public API), so they live in
// `xdr_common`.
pub use xdr_common::cxs::{ConstMaxSize, ConstWriter, const_writer, max_size};

pub mod unchecked_buf_writer;
