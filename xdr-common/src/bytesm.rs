#![allow(unused_imports)]
#![allow(dead_code)]

use crate::cfg::*;
pub use crate::error::*;

use core::fmt;
use core::marker::PhantomData;
use core::ops::Deref;

/// A byte vector with a maximum length constraint.
///
/// `BytesM<MAX>` is a wrapper around `Vec<u8>` that enforces a maximum length
/// of `MAX` bytes. This type provides hex encoding/decoding via `Display` and
/// `FromStr` implementations.
///
/// This type corresponds to XDR variable-length opaque data as defined in
/// [RFC 4506 Section 4.10](https://www.rfc-editor.org/rfc/rfc4506#section-4.10):
/// ```text
/// opaque identifier<m>;
/// opaque identifier<>;
/// ```
///
/// # Feature Variants
///
/// - **With `alloc` feature**: `Vec<u8>` is heap-allocated, supporting dynamic data
/// - **Without `alloc` feature**: `Vec<u8>` is `&'static [u8]`, for static data only
///
/// # Display Format
///
/// When displayed or serialized (with `serde` feature), bytes are encoded as
/// lowercase hexadecimal strings. Parsing accepts hex strings (requires `alloc`).
///
/// # Type Parameters
///
/// - `MAX` - Maximum number of bytes (defaults to `u32::MAX`)
///
/// # Examples
///
/// ```ignore
/// use xdr_common::BytesM;
///
/// // Create bytes with max length 100 (e.g., "opaque hash<100>")
/// let b: BytesM<100> = vec![0xde, 0xad, 0xbe, 0xef].try_into()?;
/// assert_eq!(b.to_string(), "deadbeef");
///
/// // Parse from hex string (requires alloc feature)
/// let parsed: BytesM<100> = "0123456789abcdef".parse()?;
/// assert_eq!(parsed.len(), 8);
/// ```
#[derive(Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct BytesM<const MAX: u32 = { u32::MAX }>(pub Vec<u8>);

#[cfg(all(feature = "serde", feature = "alloc"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "serde", feature = "alloc"))))]
impl<'de, const MAX: u32> Deserialize<'de> for BytesM<MAX> {
    fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(DeError::custom)
    }
}

#[cfg(all(feature = "serde", feature = "alloc"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "serde", feature = "alloc"))))]
impl<const MAX: u32> Serialize for BytesM<MAX> {
    fn serialize<S>(&self, ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if self.0.len() > Self::MAX_LEN {
            return Err(<S::Error as SerError>::custom(Error::LengthExceedsMax));
        }
        ser.collect_str(self)
    }
}

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
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl<const MAX: u32> core::str::FromStr for BytesM<MAX> {
    type Err = Error;
    fn from_str(s: &str) -> core::result::Result<Self, Self::Err> {
        if !s.len().is_multiple_of(2) {
            return Err(Error::InvalidHex);
        }
        if s.len() / 2 > MAX as usize {
            return Err(Error::LengthExceedsMax);
        }
        let mut out = Vec::with_capacity(s.len() / 2);
        let (pairs, rest) = s.as_bytes().as_chunks::<2>();
        debug_assert!(rest.is_empty()); // length is even, checked above
        for pair in pairs {
            out.push((hex_digit(pair[0])? << 4) | hex_digit(pair[1])?);
        }
        out.try_into()
    }
}

/// Decode a single ASCII hex digit (`0-9`, `a-f`, `A-F`) to its 0-15 value.
#[cfg(feature = "alloc")]
pub(crate) fn hex_digit(b: u8) -> core::result::Result<u8, Error> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(Error::InvalidHex),
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

#[cfg(feature = "schemars")]
#[cfg_attr(docsrs, doc(cfg(feature = "schemars")))]
impl<const MAX: u32> schemars::JsonSchema for BytesM<MAX> {
    fn schema_name() -> alloc::borrow::Cow<'static, str> {
        alloc::format!("BytesM<{}>", MAX).into()
    }

    fn schema_id() -> alloc::borrow::Cow<'static, str> {
        alloc::format!("xdr_common::BytesM<{}>", MAX).into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "maxLength": u64::from(MAX) * 2,
            "contentEncoding": "hex",
            "contentMediaType": "application/binary",
            "pattern": "^([0-9a-fA-F]{2})*$",
        })
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
    /// The maximum length as a compile-time constant.
    pub const MAX_LEN: usize = { MAX as usize };

    /// Returns the maximum allowed length for this byte vector.
    #[must_use]
    #[allow(clippy::unused_self)]
    pub fn max_len(&self) -> usize {
        Self::MAX_LEN
    }

    /// Returns a reference to the underlying byte vector.
    #[must_use]
    pub fn as_vec(&self) -> &Vec<u8> {
        self.as_ref()
    }

    /// Returns the bytes as a slice.
    ///
    /// Usable in `const` contexts.
    #[must_use]
    pub const fn as_slice(&self) -> &[u8] {
        #[cfg(feature = "alloc")]
        let s: &[u8] = self.0.as_slice();
        #[cfg(not(feature = "alloc"))]
        let s: &[u8] = self.0;
        s
    }
}

impl<const MAX: u32> BytesM<MAX> {
    /// Clones the bytes and returns them as a standard `Vec<u8>`.
    ///
    /// Requires the `alloc` feature.
    #[must_use]
    #[cfg(feature = "alloc")]
    #[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
    pub fn to_vec(&self) -> Vec<u8> {
        self.into()
    }

    /// Consumes `self` and returns the underlying byte vector.
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
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
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
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl<const MAX: u32> TryFrom<&Vec<u8>> for BytesM<MAX> {
    type Error = Error;

    fn try_from(v: &Vec<u8>) -> Result<Self, Error> {
        v.as_slice().try_into()
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
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
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
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
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl<const N: usize, const MAX: u32> TryFrom<BytesM<MAX>> for [u8; N] {
    type Error = BytesM<MAX>;

    fn try_from(v: BytesM<MAX>) -> core::result::Result<Self, Self::Error> {
        let s: [u8; N] = v.0.try_into().map_err(BytesM::<MAX>)?;
        Ok(s)
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
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
#[cfg_attr(docsrs, doc(cfg(not(feature = "alloc"))))]
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
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
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
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
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
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
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
#[cfg_attr(docsrs, doc(cfg(not(feature = "alloc"))))]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "alloc")]
    extern crate alloc;
    #[cfg(feature = "alloc")]
    use alloc::{string::ToString, vec};

    #[test]
    #[cfg(feature = "alloc")]
    fn test_as_slice_const() {
        // Compile check that as_slice is const-callable. With alloc, BytesM
        // values cannot be used inside const eval (Vec's destructor is not
        // const), so the const fn is exercised at runtime.
        const fn as_slice_in_const<const M: u32>(b: &BytesM<M>) -> &[u8] {
            b.as_slice()
        }
        let b: BytesM<3> = vec![0xde, 0xad].try_into().unwrap();
        assert_eq!(as_slice_in_const(&b), &[0xde, 0xad]);
    }

    #[test]
    #[cfg(not(feature = "alloc"))]
    fn test_as_slice_const() {
        const B: BytesM<3> = BytesM(&[0xde, 0xad]);
        const SLICE: &[u8] = B.as_slice();
        assert_eq!(SLICE, &[0xde, 0xad]);
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_bytesm_alloc_basic() {
        // Test basic creation with alloc feature
        let b: BytesM<100> = vec![0xde, 0xad, 0xbe, 0xef].try_into().unwrap();
        assert_eq!(b.len(), 4);
        assert_eq!(b.max_len(), 100);
        assert_eq!(b.to_string(), "deadbeef");

        // Test length exceeds max
        let too_long: Result<BytesM<3>, _> = vec![1, 2, 3, 4].try_into();
        assert!(too_long.is_err());
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn from_str_rejects_odd_length_hex() {
        let result = "abc".parse::<BytesM<100>>();
        assert_eq!(result, Err(Error::InvalidHex));
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn from_str_rejects_overlength_before_decode() {
        // 6 hex chars = 3 decoded bytes, which exceeds MAX=2.
        // Must return LengthExceedsMax without allocating the decoded vec.
        let input = "aabbcc";
        let result = input.parse::<BytesM<2>>();
        assert_eq!(result, Err(Error::LengthExceedsMax));
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn from_str_accepts_valid_hex_within_bound() {
        let result = "deadbeef".parse::<BytesM<4>>();
        assert!(result.is_ok());
        assert_eq!(
            AsRef::<[u8]>::as_ref(&result.unwrap()),
            &[0xde, 0xad, 0xbe, 0xef]
        );
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn from_str_accepts_exact_max_length() {
        // 4 hex chars = 2 decoded bytes = exactly MAX
        let result = "aabb".parse::<BytesM<2>>();
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 2);
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn from_str_rejects_invalid_hex_chars() {
        let result = "zzzz".parse::<BytesM<100>>();
        assert_eq!(result, Err(Error::InvalidHex));
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn from_str_overlength_rejects_before_validating_hex_content() {
        // Input is overlength AND contains invalid hex chars.
        // The pre-decode length guard should reject on length first,
        // without attempting to decode and discover the invalid chars.
        let result = "aabbccZZ".parse::<BytesM<1>>();
        assert_eq!(result, Err(Error::LengthExceedsMax));
    }

    #[test]
    #[cfg(feature = "schemars")]
    fn schema_pattern_rejects_odd_length_hex() {
        let schema = schemars::schema_for!(BytesM<2>);
        let obj = schema
            .as_value()
            .as_object()
            .expect("schema should be a JSON object");
        let pattern = obj
            .get("pattern")
            .expect("schema missing pattern key")
            .as_str()
            .expect("pattern should be a string");
        let re = regex_lite::Regex::new(pattern).unwrap();
        assert!(re.is_match("aabb"), "pattern should accept even-length hex");
        assert!(re.is_match(""), "pattern should accept empty string");
        assert!(!re.is_match("f"), "pattern must reject odd-length hex");
        assert!(!re.is_match("abc"), "pattern must reject odd-length hex");
    }

    #[test]
    #[cfg(all(feature = "serde", feature = "alloc"))]
    fn test_bytesm_serde_serialize_rejects_overlimit() {
        let value = BytesM::<1>(vec![0xaa, 0xbb]);
        let err = serde_json::to_value(&value).unwrap_err();
        let msg = alloc::format!("{err}");
        assert!(msg.contains("max length exceeded"), "{msg}");
    }

    #[test]
    #[cfg(all(feature = "serde", feature = "alloc"))]
    fn test_bytesm_serde_serialize_accepts_at_max() {
        let value = BytesM::<2>(vec![0xaa, 0xbb]);
        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(json, serde_json::json!("aabb"));
    }

    #[test]
    #[cfg(all(feature = "serde", feature = "alloc"))]
    fn test_bytesm_serde_roundtrip() {
        let value = BytesM::<100>(vec![0xde, 0xad, 0xbe, 0xef]);
        let json = serde_json::to_value(&value).unwrap();
        let back: BytesM<100> = serde_json::from_value(json).unwrap();
        assert_eq!(value, back);
    }

    #[test]
    #[cfg(all(feature = "serde", feature = "alloc"))]
    fn test_bytesm_serde_deserialize_rejects_invalid_hex() {
        let err = serde_json::from_value::<BytesM<100>>(serde_json::json!("zz"));
        assert!(err.is_err());
    }

    #[test]
    #[cfg(all(feature = "serde", feature = "alloc"))]
    fn test_bytesm_serde_deserialize_rejects_overlimit() {
        let err = serde_json::from_value::<BytesM<1>>(serde_json::json!("aabb"));
        assert!(err.is_err());
    }

    #[test]
    #[cfg(not(feature = "alloc"))]
    fn test_bytesm_noalloc_basic() {
        // Test basic creation without alloc feature (static data only)
        static DATA: [u8; 4] = [0xde, 0xad, 0xbe, 0xef];
        let b: BytesM<100> = (&DATA).try_into().unwrap();
        assert_eq!(b.len(), 4);
        assert_eq!(b.max_len(), 100);
        assert_eq!(AsRef::<[u8]>::as_ref(&b), &[0xde, 0xad, 0xbe, 0xef]);

        // Test length exceeds max
        static LONG_DATA: [u8; 4] = [1, 2, 3, 4];
        let too_long: Result<BytesM<3>, _> = (&LONG_DATA).try_into();
        assert!(too_long.is_err());
    }
}
