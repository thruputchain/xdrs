// Copied from https://github.com/stellar/xdrgen/blob/master/lib/xdrgen/generators/rust/src/types.rs

#![allow(unused_imports)]
#![allow(dead_code)]

#[cfg(all(feature = "serde", feature = "alloc"))]
pub use serde::de::{Deserialize, Deserializer, Error as DeError, SeqAccess, Visitor};
#[cfg(all(feature = "serde", feature = "alloc"))]
pub use serde::ser::{Serialize, Serializer};

// When feature alloc is turned off use static lifetime Box and Vec types.
#[cfg(not(feature = "alloc"))]
pub mod noalloc {
    pub mod boxed {
        pub type Box<T> = &'static T;
    }
    pub mod vec {
        pub type Vec<T> = &'static [T];
    }
}
#[cfg(not(feature = "alloc"))]
pub use noalloc::{boxed::Box, vec::Vec};

// When feature std is turned off, but feature alloc is turned on import the
// alloc crate and use its Box and Vec types.
#[cfg(all(not(feature = "std"), feature = "alloc"))]
pub extern crate alloc;
#[cfg(feature = "alloc")]
pub use alloc::{
    borrow::ToOwned,
    boxed::Box,
    string::{FromUtf8Error, String},
    vec::Vec,
};

#[cfg(feature = "std")]
pub use std::{error, io};
