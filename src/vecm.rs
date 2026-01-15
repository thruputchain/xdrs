#![allow(unused_imports)]
#![allow(dead_code)]

pub use crate::cfg::*;
pub use crate::error::*;

use core::fmt;
use core::marker::PhantomData;
use core::ops::Deref;

/// A vector with a maximum length constraint.
///
/// `VecM<T, MAX>` is a wrapper around `Vec<T>` that enforces a maximum length
/// of `MAX` elements. Attempting to create a `VecM` from a `Vec` that exceeds
/// this length will result in an [`Error::LengthExceedsMax`].
///
/// This type corresponds to XDR variable-length arrays as defined in
/// [RFC 4506 Section 4.13](https://www.rfc-editor.org/rfc/rfc4506#section-4.13):
/// ```text
/// type-name identifier<m>;
/// type-name identifier<>;
/// ```
///
/// # Type Parameters
///
/// - `T` - The element type
/// - `MAX` - Maximum number of elements (defaults to `u32::MAX`)
///
/// # Examples
///
/// ```ignore
/// use xdr_common::VecM;
///
/// // Create a vector with max 5 elements (e.g., "int values<5>")
/// let v: VecM<u32, 5> = vec![1, 2, 3].try_into()?;
/// assert_eq!(v.len(), 3);
/// assert_eq!(v.max_len(), 5);
///
/// // This would fail because it exceeds MAX
/// let too_long: Result<VecM<u32, 5>, _> = vec![1, 2, 3, 4, 5, 6].try_into();
/// assert!(too_long.is_err());
/// ```
#[cfg(feature = "alloc")]
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct VecM<T, const MAX: u32 = { u32::MAX }>(pub Vec<T>);

/// A vector with a maximum length constraint (no_std version).
///
/// See [`VecM`] documentation for details. This version requires `T: 'static`
/// for compatibility with no_std environments.
#[cfg(not(feature = "alloc"))]
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct VecM<T, const MAX: u32 = { u32::MAX }>(pub Vec<T>)
where
    T: 'static;

#[cfg(all(feature = "serde", feature = "alloc"))]
impl<T, const MAX: u32> Serialize for VecM<T, MAX>
where
    T: Serialize,
{
    fn serialize<S>(&self, ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(ser)
    }
}

#[cfg(all(feature = "serde", feature = "alloc"))]
impl<'de, T, const MAX: u32> Deserialize<'de> for VecM<T, MAX>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(de: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct VecMVisitor<T, const MAX: u32>(PhantomData<T>);

        impl<'de, T, const MAX: u32> Visitor<'de> for VecMVisitor<T, MAX>
        where
            T: Deserialize<'de>,
        {
            type Value = VecM<T, MAX>;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a sequence with length at most {}", MAX)
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut buf = Vec::new();
                if let Some(len) = seq.size_hint() {
                    if len > VecM::<T, MAX>::MAX_LEN {
                        return Err(<A::Error as DeError>::invalid_length(len, &self));
                    }
                    buf.reserve(len);
                }

                while let Some(value) = seq.next_element::<T>()? {
                    if buf.len() >= VecM::<T, MAX>::MAX_LEN {
                        return Err(<A::Error as DeError>::invalid_length(buf.len() + 1, &self));
                    }
                    buf.push(value);
                }

                VecM::<T, MAX>::try_from(buf).map_err(|e| <A::Error as DeError>::custom(e))
            }
        }

        de.deserialize_seq(VecMVisitor::<T, MAX>(PhantomData))
    }
}

impl<T, const MAX: u32> Deref for VecM<T, MAX> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T, const MAX: u32> Default for VecM<T, MAX> {
    fn default() -> Self {
        Self(Vec::default())
    }
}

impl<T, const MAX: u32> VecM<T, MAX> {
    /// The maximum length as a compile-time constant.
    pub const MAX_LEN: usize = { MAX as usize };

    /// Returns the maximum allowed length for this vector.
    #[must_use]
    #[allow(clippy::unused_self)]
    pub fn max_len(&self) -> usize {
        Self::MAX_LEN
    }

    /// Returns a reference to the underlying vector.
    #[must_use]
    pub fn as_vec(&self) -> &Vec<T> {
        self.as_ref()
    }
}

impl<T: Clone, const MAX: u32> VecM<T, MAX> {
    /// Clones the vector and returns it as a standard `Vec<T>`.
    ///
    /// Requires the `alloc` feature.
    #[must_use]
    #[cfg(feature = "alloc")]
    pub fn to_vec(&self) -> Vec<T> {
        self.into()
    }

    /// Consumes `self` and returns the underlying vector.
    #[must_use]
    pub fn into_vec(self) -> Vec<T> {
        self.into()
    }
}

impl<T: Clone> VecM<T, 1> {
    /// Converts a `VecM<T, 1>` to `Option<T>`.
    ///
    /// Returns `Some(T)` if the vector contains one element, `None` otherwise.
    /// This is useful for optional XDR fields.
    #[must_use]
    pub fn to_option(&self) -> Option<T> {
        if self.len() > 0 {
            Some(self.0[0].clone())
        } else {
            None
        }
    }
}

#[cfg(not(feature = "alloc"))]
impl<T: Clone> From<VecM<T, 1>> for Option<T> {
    fn from(v: VecM<T, 1>) -> Self {
        v.to_option()
    }
}

#[cfg(feature = "alloc")]
impl<T> VecM<T, 1> {
    /// Consumes `self` and converts it to `Option<T>`.
    ///
    /// Returns `Some(T)` if the vector contains one element, `None` otherwise.
    /// This is useful for optional XDR fields.
    ///
    /// Requires the `alloc` feature.
    #[must_use]
    pub fn into_option(mut self) -> Option<T> {
        self.0.drain(..).next()
    }
}

#[cfg(feature = "alloc")]
impl<T> From<VecM<T, 1>> for Option<T> {
    fn from(v: VecM<T, 1>) -> Self {
        v.into_option()
    }
}

impl<T, const MAX: u32> TryFrom<Vec<T>> for VecM<T, MAX> {
    type Error = Error;

    fn try_from(v: Vec<T>) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(VecM(v))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

impl<T, const MAX: u32> From<VecM<T, MAX>> for Vec<T> {
    fn from(v: VecM<T, MAX>) -> Self {
        v.0
    }
}

#[cfg(feature = "alloc")]
impl<T: Clone, const MAX: u32> From<&VecM<T, MAX>> for Vec<T> {
    fn from(v: &VecM<T, MAX>) -> Self {
        v.0.clone()
    }
}

impl<T, const MAX: u32> AsRef<Vec<T>> for VecM<T, MAX> {
    fn as_ref(&self) -> &Vec<T> {
        &self.0
    }
}

#[cfg(feature = "alloc")]
impl<T: Clone, const MAX: u32> TryFrom<&Vec<T>> for VecM<T, MAX> {
    type Error = Error;

    fn try_from(v: &Vec<T>) -> Result<Self, Error> {
        v.as_slice().try_into()
    }
}

#[cfg(feature = "alloc")]
impl<T: Clone, const MAX: u32> TryFrom<&[T]> for VecM<T, MAX> {
    type Error = Error;

    fn try_from(v: &[T]) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(VecM(v.to_vec()))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

impl<T, const MAX: u32> AsRef<[T]> for VecM<T, MAX> {
    #[cfg(feature = "alloc")]
    fn as_ref(&self) -> &[T] {
        self.0.as_ref()
    }
    #[cfg(not(feature = "alloc"))]
    fn as_ref(&self) -> &[T] {
        self.0
    }
}

#[cfg(feature = "alloc")]
impl<T: Clone, const N: usize, const MAX: u32> TryFrom<[T; N]> for VecM<T, MAX> {
    type Error = Error;

    fn try_from(v: [T; N]) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(VecM(v.to_vec()))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

#[cfg(feature = "alloc")]
impl<T: Clone, const N: usize, const MAX: u32> TryFrom<VecM<T, MAX>> for [T; N] {
    type Error = VecM<T, MAX>;

    fn try_from(v: VecM<T, MAX>) -> core::result::Result<Self, Self::Error> {
        let s: [T; N] = v.0.try_into().map_err(|v: Vec<T>| VecM::<T, MAX>(v))?;
        Ok(s)
    }
}

#[cfg(feature = "alloc")]
impl<T: Clone, const N: usize, const MAX: u32> TryFrom<&[T; N]> for VecM<T, MAX> {
    type Error = Error;

    fn try_from(v: &[T; N]) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(VecM(v.to_vec()))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

#[cfg(not(feature = "alloc"))]
impl<T: Clone, const N: usize, const MAX: u32> TryFrom<&'static [T; N]> for VecM<T, MAX> {
    type Error = Error;

    fn try_from(v: &'static [T; N]) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(VecM(v))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

#[cfg(feature = "alloc")]
impl<const MAX: u32> TryFrom<&String> for VecM<u8, MAX> {
    type Error = Error;

    fn try_from(v: &String) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(VecM(v.as_bytes().to_vec()))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

#[cfg(feature = "alloc")]
impl<const MAX: u32> TryFrom<String> for VecM<u8, MAX> {
    type Error = Error;

    fn try_from(v: String) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(VecM(v.into()))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

#[cfg(feature = "alloc")]
impl<const MAX: u32> TryFrom<&str> for VecM<u8, MAX> {
    type Error = Error;

    fn try_from(v: &str) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(VecM(v.into()))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

#[cfg(not(feature = "alloc"))]
impl<const MAX: u32> TryFrom<&'static str> for VecM<u8, MAX> {
    type Error = Error;

    fn try_from(v: &'static str) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(VecM(v.as_bytes()))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}
