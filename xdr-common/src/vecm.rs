#![allow(unused_imports)]
#![allow(dead_code)]

use crate::cfg::*;
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
/// # Feature Variants
///
/// - **With `alloc` feature**: `Vec<T>` is heap-allocated, supporting dynamic data
/// - **Without `alloc` feature**: `Vec<T>` is `&'static [T]`, for static data only (requires `T: 'static`)
///
/// # Type Parameters
///
/// - `T` - The element type
/// - `MAX` - Maximum number of elements (defaults to `u32::MAX`)
///
/// # Schema note — `VecM<u8>` vs `BytesM`
///
/// `VecM<u8, MAX>` and `BytesM<MAX>` share the same XDR wire format
/// (variable-length opaque) and the same `xdr-repr` metadata, but their
/// `schemars` JSON schemas differ: `BytesM` advertises a hex-encoded string
/// while `VecM<u8>` advertises a JSON array of integers. This is intentional —
/// each schema faithfully describes its own serde JSON shape. If you need the
/// hex-string schema, use `BytesM` directly.
///
/// The same applies to `VecM<Vec<u8>, MAX>`: the inner `Vec<u8>` element
/// schema is the standard schemars array-of-integers, not the XDR opaque
/// representation. Prefer `VecM<BytesM<INNER>, MAX>` when the JSON schema
/// must reflect opaque byte semantics.
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
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[allow(private_bounds)]
pub struct VecM<T, const MAX: u32 = { u32::MAX }>(pub Vec<T>)
where
    T: StaticIfNoAlloc;

// Helper trait to conditionally require 'static for no_alloc
#[cfg(feature = "alloc")]
pub trait StaticIfNoAlloc {}
#[cfg(feature = "alloc")]
impl<T> StaticIfNoAlloc for T {}

#[cfg(not(feature = "alloc"))]
pub trait StaticIfNoAlloc: 'static {}
#[cfg(not(feature = "alloc"))]
impl<T: 'static> StaticIfNoAlloc for T {}

#[cfg(all(feature = "serde", feature = "alloc"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "serde", feature = "alloc"))))]
impl<T, const MAX: u32> Serialize for VecM<T, MAX>
where
    T: Serialize,
{
    fn serialize<S>(&self, ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if self.0.len() > Self::MAX_LEN {
            return Err(<S::Error as SerError>::custom(Error::LengthExceedsMax));
        }
        self.0.serialize(ser)
    }
}

#[cfg(all(feature = "serde", feature = "alloc"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "serde", feature = "alloc"))))]
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

                while buf.len() < VecM::<T, MAX>::MAX_LEN {
                    match seq.next_element::<T>()? {
                        Some(value) => buf.push(value),
                        None => return Ok(VecM(buf)),
                    }
                }
                if seq.next_element::<IgnoredAny>()?.is_some() {
                    return Err(match buf.len().checked_add(1) {
                        Some(overlimit) => <A::Error as DeError>::invalid_length(overlimit, &self),
                        None => DeError::custom(format_args!(
                            "sequence length exceeds maximum of {MAX}"
                        )),
                    });
                }
                Ok(VecM(buf))
            }
        }

        de.deserialize_seq(VecMVisitor::<T, MAX>(PhantomData))
    }
}

#[cfg(feature = "schemars")]
#[cfg_attr(docsrs, doc(cfg(feature = "schemars")))]
impl<T: schemars::JsonSchema, const MAX: u32> schemars::JsonSchema for VecM<T, MAX> {
    fn schema_name() -> alloc::borrow::Cow<'static, str> {
        alloc::format!("VecM<{}, {}>", T::schema_name(), MAX).into()
    }

    fn schema_id() -> alloc::borrow::Cow<'static, str> {
        alloc::format!("xdr_common::VecM<{}, {}>", T::schema_id(), MAX).into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let items = generator.subschema_for::<T>();
        schemars::json_schema!({
            "type": "array",
            "items": items,
            "maxItems": MAX,
        })
    }
}

impl<T: StaticIfNoAlloc, const MAX: u32> Deref for VecM<T, MAX> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: StaticIfNoAlloc, const MAX: u32> Default for VecM<T, MAX> {
    fn default() -> Self {
        Self(Vec::default())
    }
}

#[allow(private_bounds)]
impl<T: StaticIfNoAlloc, const MAX: u32> VecM<T, MAX> {
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

#[allow(private_bounds)]
impl<T: Clone + StaticIfNoAlloc, const MAX: u32> VecM<T, MAX> {
    /// Clones the vector and returns it as a standard `Vec<T>`.
    ///
    /// Requires the `alloc` feature.
    #[must_use]
    #[cfg(feature = "alloc")]
    #[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
    pub fn to_vec(&self) -> Vec<T> {
        self.into()
    }

    /// Consumes `self` and returns the underlying vector.
    #[must_use]
    pub fn into_vec(self) -> Vec<T> {
        self.into()
    }
}

#[allow(private_bounds)]
impl<T: Clone + StaticIfNoAlloc> VecM<T, 1> {
    /// Converts a `VecM<T, 1>` to `Option<T>`.
    ///
    /// Returns `Some(T)` if the vector contains one element, `None` otherwise.
    /// This is useful for optional XDR fields.
    #[must_use]
    pub fn to_option(&self) -> Option<T> {
        self.0.first().cloned()
    }
}

#[cfg(not(feature = "alloc"))]
#[cfg_attr(docsrs, doc(cfg(not(feature = "alloc"))))]
impl<T: Clone + 'static> From<VecM<T, 1>> for Option<T> {
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
    #[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
    pub fn into_option(mut self) -> Option<T> {
        self.0.drain(..).next()
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl<T> From<VecM<T, 1>> for Option<T> {
    fn from(v: VecM<T, 1>) -> Self {
        v.into_option()
    }
}

impl<T: StaticIfNoAlloc, const MAX: u32> TryFrom<Vec<T>> for VecM<T, MAX> {
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

impl<T: StaticIfNoAlloc, const MAX: u32> From<VecM<T, MAX>> for Vec<T> {
    fn from(v: VecM<T, MAX>) -> Self {
        v.0
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl<T: Clone, const MAX: u32> From<&VecM<T, MAX>> for Vec<T> {
    fn from(v: &VecM<T, MAX>) -> Self {
        v.0.clone()
    }
}

impl<T: StaticIfNoAlloc, const MAX: u32> AsRef<Vec<T>> for VecM<T, MAX> {
    fn as_ref(&self) -> &Vec<T> {
        &self.0
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl<T: Clone, const MAX: u32> TryFrom<&Vec<T>> for VecM<T, MAX> {
    type Error = Error;

    fn try_from(v: &Vec<T>) -> Result<Self, Error> {
        v.as_slice().try_into()
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
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

impl<T: StaticIfNoAlloc, const MAX: u32> AsRef<[T]> for VecM<T, MAX> {
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
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
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
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl<T: Clone, const N: usize, const MAX: u32> TryFrom<VecM<T, MAX>> for [T; N] {
    type Error = VecM<T, MAX>;

    fn try_from(v: VecM<T, MAX>) -> core::result::Result<Self, Self::Error> {
        let s: [T; N] = v.0.try_into().map_err(|v: Vec<T>| VecM::<T, MAX>(v))?;
        Ok(s)
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
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
#[cfg_attr(docsrs, doc(cfg(not(feature = "alloc"))))]
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
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
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
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
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
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
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
#[cfg_attr(docsrs, doc(cfg(not(feature = "alloc"))))]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "alloc")]
    extern crate alloc;
    #[cfg(feature = "alloc")]
    use alloc::vec;

    #[test]
    #[cfg(feature = "alloc")]
    fn test_vecm_alloc_basic() {
        // Test basic creation with alloc feature
        let v: VecM<u32, 10> = vec![1, 2, 3].try_into().unwrap();
        assert_eq!(v.len(), 3);
        assert_eq!(v.max_len(), 10);
        assert_eq!(v[0], 1);

        // Test length exceeds max
        let too_long: Result<VecM<u32, 5>, _> = vec![1, 2, 3, 4, 5, 6].try_into();
        assert!(too_long.is_err());
    }

    #[test]
    #[cfg(all(feature = "serde", feature = "alloc"))]
    fn test_vecm_serde_deser_rejects_overlimit() {
        let ok: VecM<u32, 3> = serde_json::from_str("[1,2,3]").unwrap();
        assert_eq!(ok.as_vec(), &vec![1, 2, 3]);

        let under: VecM<u32, 5> = serde_json::from_str("[1]").unwrap();
        assert_eq!(under.as_vec(), &vec![1]);

        let empty: VecM<u32, 0> = serde_json::from_str("[]").unwrap();
        assert!(empty.is_empty());

        let err = serde_json::from_str::<VecM<u32, 2>>("[1,2,3]").unwrap_err();
        let msg = alloc::format!("{err}");
        assert!(msg.contains("length at most 2"), "{msg}");

        let err = serde_json::from_str::<VecM<u32, 0>>("[1]").unwrap_err();
        let msg = alloc::format!("{err}");
        assert!(msg.contains("length at most 0"), "{msg}");
    }

    #[test]
    #[cfg(all(feature = "serde", feature = "alloc"))]
    fn test_vecm_serde_serialize_rejects_overlimit() {
        let value = VecM::<u32, 1>(vec![1, 2]);
        let err = serde_json::to_value(&value).unwrap_err();
        let msg = alloc::format!("{err}");
        assert!(msg.contains("max length exceeded"), "{msg}");
    }

    #[test]
    #[cfg(all(feature = "serde", feature = "alloc"))]
    fn test_vecm_serde_serialize_accepts_at_max() {
        let value = VecM::<u32, 2>(vec![1, 2]);
        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(json, serde_json::json!([1, 2]));
    }

    #[test]
    #[cfg(not(feature = "alloc"))]
    fn test_vecm_noalloc_basic() {
        // Test basic creation without alloc feature (static data only)
        static DATA: [u32; 3] = [1, 2, 3];
        let v: VecM<u32, 10> = (&DATA).try_into().unwrap();
        assert_eq!(v.len(), 3);
        assert_eq!(v.max_len(), 10);
        assert_eq!(v[0], 1);

        // Test length exceeds max
        static LONG_DATA: [u32; 6] = [1, 2, 3, 4, 5, 6];
        let too_long: Result<VecM<u32, 5>, _> = (&LONG_DATA).try_into();
        assert!(too_long.is_err());
    }
}
