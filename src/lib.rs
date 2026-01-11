#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

mod cfg;

mod error;
pub use error::*;

mod vecm;
pub use vecm::*;

mod bytesm;
pub use bytesm::*;
