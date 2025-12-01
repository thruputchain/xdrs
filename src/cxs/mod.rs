//! CXS (_Compact XDR Serialization_) implementation of XDR serialization

mod utils;
pub use utils::*;

mod limits;

pub mod buf_reader;
pub mod into_buf_writer;

#[cfg(feature = "alloc")]
pub mod buf_writer;

mod put;
