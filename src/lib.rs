#![no_std]

#[cfg(feature = "std")]
extern crate std;

mod cfg;

mod error;
pub use error::*;

mod vecm;
pub use vecm::*;

mod bytesm;
pub use bytesm::*;

mod stringm;
pub use stringm::*;
