#![doc = include_str!("../../CXS.md")]

mod utils;
pub use utils::*;

mod limits;

pub mod buf_reader;
pub mod into_buf_writer;

#[cfg(feature = "alloc")]
pub mod buf_writer;

mod put;

mod const_writer;
pub use const_writer::ConstWriter;
