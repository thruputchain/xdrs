#![allow(unused_imports)]
#![allow(dead_code)]

pub use crate::cfg::*;

use core::error;

/// Errors that can occur when working with XDR types.
///
/// This error type can be compared via `PartialEq`.
#[derive(Debug)]
pub enum Error {
    /// The XDR value is invalid or malformed.
    Invalid,
    /// The XDR value or operation is not supported.
    Unsupported,
    /// The length of the value exceeds the maximum allowed length.
    ///
    /// This occurs when trying to create a constrained type (e.g., `VecM<T, MAX>`)
    /// from data that exceeds the `MAX` length parameter.
    LengthExceedsMax,
    /// The length of the value does not match the expected length.
    LengthMismatch,
    /// XDR padding contains non-zero bytes (XDR requires zero padding).
    NonZeroPadding,
    /// The hex string is invalid.
    ///
    /// This error occurs when parsing hex-encoded byte strings.
    #[cfg(feature = "alloc")]
    InvalidHex,
    /// The byte sequence is not valid UTF-8.
    ///
    /// This occurs when attempting to convert a `StringM` to a UTF-8 `String`.
    InvalidUTF8,
    /// The depth limit was exceeded during processing.
    DepthLimitExceeded,
    /// The length limit was exceeded during processing.
    LengthLimitExceeded,
}

impl PartialEq for Error {
    fn eq(&self, other: &Self) -> bool {
        core::mem::discriminant(self) == core::mem::discriminant(other)
    }
}

impl error::Error for Error {}

use core::array::TryFromSliceError;
use core::fmt;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Invalid => write!(f, "xdr value invalid"),
            Error::Unsupported => write!(f, "xdr value unsupported"),
            Error::LengthExceedsMax => {
                write!(f, "xdr value max length exceeded")
            }
            Error::LengthMismatch => {
                write!(f, "xdr value length does not match")
            }
            Error::NonZeroPadding => {
                write!(f, "xdr padding contains non-zero bytes")
            }
            #[cfg(feature = "alloc")]
            Error::InvalidHex => write!(f, "hex invalid"),
            Error::InvalidUTF8 => write!(f, "utf8 invalid"),
            Error::DepthLimitExceeded => write!(f, "depth limit exceeded"),
            Error::LengthLimitExceeded => write!(f, "length limit exceeded"),
        }
    }
}

impl From<TryFromSliceError> for Error {
    fn from(_: TryFromSliceError) -> Error {
        Error::LengthMismatch
    }
}

impl From<core::str::Utf8Error> for Error {
    fn from(_: core::str::Utf8Error) -> Self {
        Error::InvalidUTF8
    }
}

#[cfg(feature = "alloc")]
impl From<FromUtf8Error> for Error {
    fn from(_: FromUtf8Error) -> Self {
        Error::InvalidUTF8
    }
}
