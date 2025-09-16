//! An interface for serializing XDR primitives,
//! which certain formats may implement.  The
//! formats must:
//! - Serialize structs simply by concatenating their fields
//! - Serialize arrays simply by concatenating their elements

pub mod reader;
pub mod reader_state;

pub mod writer;
