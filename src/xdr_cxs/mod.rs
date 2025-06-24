//! CXS implementation of XDR serialization

mod utils;
pub use utils::*;

pub mod into_buf_writer;

pub mod buf_reader;

#[cfg(feature = "alloc")]
pub mod buf_writer;

pub(crate) mod put;
