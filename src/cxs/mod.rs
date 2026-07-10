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

pub mod const_writer;
pub use const_writer::ConstWriter;

pub mod max_size;
pub use max_size::ConstMaxSize;

pub mod unchecked_buf_writer;
