#![allow(unused_imports)]
#![allow(dead_code)]

pub use crate::cfg::*;
pub use crate::error::*;

use core::fmt;
use core::marker::PhantomData;
use core::ops::Deref;

#[cfg(feature = "alloc")]
#[derive(Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "serde",
    derive(serde_with::SerializeDisplay, serde_with::DeserializeFromStr)
)]
pub struct BytesM<const MAX: u32 = { u32::MAX }>(pub Vec<u8>);

#[cfg(not(feature = "alloc"))]
#[derive(Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct BytesM<const MAX: u32 = { u32::MAX }>(pub Vec<u8>);

#[cfg(all(feature = "serde", feature = "alloc"))]
use hex::{FromHex, ToHex};

impl<const MAX: u32> core::fmt::Display for BytesM<MAX> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        #[cfg(feature = "alloc")]
        let v = &self.0;
        #[cfg(not(feature = "alloc"))]
        let v = self.0;
        for b in v {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

#[cfg(feature = "alloc")]
impl<const MAX: u32> core::str::FromStr for BytesM<MAX> {
    type Err = Error;
    fn from_str(s: &str) -> core::result::Result<Self, Self::Err> {
        hex::decode(s).map_err(|_| Error::InvalidHex)?.try_into()
    }
}

impl<const MAX: u32> core::fmt::Debug for BytesM<MAX> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        #[cfg(feature = "alloc")]
        let v = &self.0;
        #[cfg(not(feature = "alloc"))]
        let v = self.0;
        write!(f, "BytesM(")?;
        for b in v {
            write!(f, "{b:02x}")?;
        }
        write!(f, ")")?;
        Ok(())
    }
}

impl<const MAX: u32> Deref for BytesM<MAX> {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<const MAX: u32> Default for BytesM<MAX> {
    fn default() -> Self {
        Self(Vec::default())
    }
}

impl<const MAX: u32> BytesM<MAX> {
    pub const MAX_LEN: usize = { MAX as usize };

    #[must_use]
    #[allow(clippy::unused_self)]
    pub fn max_len(&self) -> usize {
        Self::MAX_LEN
    }

    #[must_use]
    pub fn as_vec(&self) -> &Vec<u8> {
        self.as_ref()
    }
}

impl<const MAX: u32> BytesM<MAX> {
    #[must_use]
    #[cfg(feature = "alloc")]
    pub fn to_vec(&self) -> Vec<u8> {
        self.into()
    }

    #[must_use]
    pub fn into_vec(self) -> Vec<u8> {
        self.into()
    }
}

impl<const MAX: u32> TryFrom<Vec<u8>> for BytesM<MAX> {
    type Error = Error;

    fn try_from(v: Vec<u8>) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(BytesM(v))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

impl<const MAX: u32> From<BytesM<MAX>> for Vec<u8> {
    fn from(v: BytesM<MAX>) -> Self {
        v.0
    }
}

#[cfg(feature = "alloc")]
impl<const MAX: u32> From<&BytesM<MAX>> for Vec<u8> {
    fn from(v: &BytesM<MAX>) -> Self {
        v.0.clone()
    }
}

impl<const MAX: u32> AsRef<Vec<u8>> for BytesM<MAX> {
    fn as_ref(&self) -> &Vec<u8> {
        &self.0
    }
}

#[cfg(feature = "alloc")]
impl<const MAX: u32> TryFrom<&Vec<u8>> for BytesM<MAX> {
    type Error = Error;

    fn try_from(v: &Vec<u8>) -> Result<Self, Error> {
        v.as_slice().try_into()
    }
}

#[cfg(feature = "alloc")]
impl<const MAX: u32> TryFrom<&[u8]> for BytesM<MAX> {
    type Error = Error;

    fn try_from(v: &[u8]) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(BytesM(v.to_vec()))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

impl<const MAX: u32> AsRef<[u8]> for BytesM<MAX> {
    #[cfg(feature = "alloc")]
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
    #[cfg(not(feature = "alloc"))]
    fn as_ref(&self) -> &[u8] {
        self.0
    }
}

#[cfg(feature = "alloc")]
impl<const N: usize, const MAX: u32> TryFrom<[u8; N]> for BytesM<MAX> {
    type Error = Error;

    fn try_from(v: [u8; N]) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(BytesM(v.to_vec()))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

#[cfg(feature = "alloc")]
impl<const N: usize, const MAX: u32> TryFrom<BytesM<MAX>> for [u8; N] {
    type Error = BytesM<MAX>;

    fn try_from(v: BytesM<MAX>) -> core::result::Result<Self, Self::Error> {
        let s: [u8; N] = v.0.try_into().map_err(BytesM::<MAX>)?;
        Ok(s)
    }
}

#[cfg(feature = "alloc")]
impl<const N: usize, const MAX: u32> TryFrom<&[u8; N]> for BytesM<MAX> {
    type Error = Error;

    fn try_from(v: &[u8; N]) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(BytesM(v.to_vec()))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

#[cfg(not(feature = "alloc"))]
impl<const N: usize, const MAX: u32> TryFrom<&'static [u8; N]> for BytesM<MAX> {
    type Error = Error;

    fn try_from(v: &'static [u8; N]) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(BytesM(v))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

#[cfg(feature = "alloc")]
impl<const MAX: u32> TryFrom<&String> for BytesM<MAX> {
    type Error = Error;

    fn try_from(v: &String) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(BytesM(v.as_bytes().to_vec()))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

#[cfg(feature = "alloc")]
impl<const MAX: u32> TryFrom<String> for BytesM<MAX> {
    type Error = Error;

    fn try_from(v: String) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(BytesM(v.into()))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

#[cfg(feature = "alloc")]
impl<const MAX: u32> TryFrom<&str> for BytesM<MAX> {
    type Error = Error;

    fn try_from(v: &str) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(BytesM(v.into()))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

#[cfg(not(feature = "alloc"))]
impl<const MAX: u32> TryFrom<&'static str> for BytesM<MAX> {
    type Error = Error;

    fn try_from(v: &'static str) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(BytesM(v.as_bytes()))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}
