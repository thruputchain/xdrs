#![allow(unused_imports)]
#![allow(dead_code)]

use crate::cfg::*;
pub use crate::error::*;

use core::fmt;
use core::marker::PhantomData;
use core::ops::Deref;

/// A string type that stores arbitrary bytes with a maximum length constraint.
///
/// `StringM<MAX>` is a wrapper around `Vec<u8>` that represents string data,
/// which may or may not be valid UTF-8. This type enforces a maximum length
/// of `MAX` bytes.
///
/// This type corresponds to XDR strings as defined in
/// [RFC 4506 Section 4.11](https://www.rfc-editor.org/rfc/rfc4506#section-4.11):
/// ```text
/// string identifier<m>;
/// string identifier<>;
/// ```
///
/// # Character Encoding
///
/// **Note:** The XDR standard (RFC 4506) specifies that strings should contain
/// ASCII characters. However, this implementation does **not** enforce ASCII
/// encoding, nor does it strictly enforce UTF-8. The underlying storage is a
/// raw byte vector (`Vec<u8>`) that accepts arbitrary bytes. Users are
/// responsible for ensuring their data meets the requirements of their specific
/// XDR protocol.
///
/// # UTF-8 Conversion
///
/// - Fallible conversion to/from UTF-8 `String`: [`TryFrom`]/[`TryInto`]/[`StringM::to_utf8_string`]
/// - Lossy conversion to UTF-8 `String`: [`StringM::to_utf8_string_lossy`]
///
/// # Feature Variants
///
/// - **With `alloc` feature**: `Vec<u8>` is heap-allocated, supporting dynamic data
/// - **Without `alloc` feature**: `Vec<u8>` is `&'static [u8]`, for static data only
///
/// # Display Format and Serde Representation
///
/// When displayed or serialized (with `serde` feature), strings are shown
/// as escaped printable ASCII using [`core::fmt::Display`]/`ToString`. Non-ASCII
/// bytes are hex-escaped (e.g. `\xc3\xa9` for `é`).
///
/// Deserialization via [`core::str::FromStr`] is intentionally permissive: it
/// accepts both the canonical escaped form and raw UTF-8 input. This means
/// `"café"` and `"caf\xc3\xa9"` both deserialize to the same byte sequence,
/// but serialization always produces the escaped form.
///
/// # Type Parameters
///
/// - `MAX` - Maximum number of bytes (defaults to `u32::MAX`)
///
/// # Examples
///
/// ```ignore
/// use xdr_common::StringM;
///
/// // Create a string with max length 50 (e.g., "string name<50>")
/// let s: StringM<50> = "hello".try_into()?;
/// assert_eq!(s.to_utf8_string()?, "hello");
///
/// // Handle arbitrary bytes (may not be valid UTF-8)
/// let bytes: StringM<100> = vec![0xFF, 0xFE].try_into()?;
/// let lossy = bytes.to_utf8_string_lossy(); // Replaces invalid chars
/// ```
#[derive(Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    all(feature = "serde", feature = "alloc"),
    derive(serde_with::DeserializeFromStr)
)]
pub struct StringM<const MAX: u32 = { u32::MAX }>(pub Vec<u8>);

#[cfg(all(feature = "serde", feature = "alloc"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "serde", feature = "alloc"))))]
impl<const MAX: u32> Serialize for StringM<MAX> {
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

impl<const MAX: u32> core::fmt::Display for StringM<MAX> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        #[cfg(feature = "alloc")]
        let v = &self.0;
        #[cfg(not(feature = "alloc"))]
        let v = self.0;
        for b in escape_bytes::Escape::new(v) {
            write!(f, "{}", b as char)?;
        }
        Ok(())
    }
}

impl<const MAX: u32> core::fmt::Debug for StringM<MAX> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        #[cfg(feature = "alloc")]
        let v = &self.0;
        #[cfg(not(feature = "alloc"))]
        let v = self.0;
        write!(f, "StringM(")?;
        for b in escape_bytes::Escape::new(v) {
            write!(f, "{}", b as char)?;
        }
        write!(f, ")")?;
        Ok(())
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl<const MAX: u32> core::str::FromStr for StringM<MAX> {
    type Err = Error;
    fn from_str(s: &str) -> core::result::Result<Self, Self::Err> {
        // Each escaped byte is at most 4 input chars (\xNN), so if the input
        // exceeds MAX * 4, the unescaped output will certainly exceed MAX.
        if s.len() > Self::MAX_LEN.saturating_mul(4) {
            return Err(Error::LengthExceedsMax);
        }
        escape_bytes::unescape(s.as_bytes())
            .map_err(|_| Error::Invalid)?
            .try_into()
    }
}

#[cfg(feature = "schemars")]
#[cfg_attr(docsrs, doc(cfg(feature = "schemars")))]
impl<const MAX: u32> schemars::JsonSchema for StringM<MAX> {
    fn schema_name() -> alloc::borrow::Cow<'static, str> {
        alloc::format!("StringM<{}>", MAX).into()
    }

    fn schema_id() -> alloc::borrow::Cow<'static, str> {
        alloc::format!("xdr_common::StringM<{}>", MAX).into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "x-max-unescaped-bytes": MAX,
            "description": "XDR string with escape_bytes encoding. x-max-unescaped-bytes is the byte limit of the unescaped XDR payload, not the character count of the JSON value.",
        })
    }
}

impl<const MAX: u32> Deref for StringM<MAX> {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<const MAX: u32> Default for StringM<MAX> {
    fn default() -> Self {
        Self(Vec::default())
    }
}

impl<const MAX: u32> StringM<MAX> {
    /// The maximum length as a compile-time constant.
    pub const MAX_LEN: usize = { MAX as usize };

    /// Returns the maximum allowed length for this string.
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
}

impl<const MAX: u32> StringM<MAX> {
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

impl<const MAX: u32> StringM<MAX> {
    /// Converts the string to a UTF-8 `String`.
    ///
    /// Returns [`Error::InvalidUTF8`] if the bytes are not valid UTF-8.
    ///
    /// Requires the `alloc` feature.
    #[cfg(feature = "alloc")]
    #[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
    pub fn to_utf8_string(&self) -> Result<String, Error> {
        self.try_into()
    }

    /// Consumes `self` and converts it to a UTF-8 `String`.
    ///
    /// Returns [`Error::InvalidUTF8`] if the bytes are not valid UTF-8.
    ///
    /// Requires the `alloc` feature.
    #[cfg(feature = "alloc")]
    #[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
    pub fn into_utf8_string(self) -> Result<String, Error> {
        self.try_into()
    }

    /// Converts the string to a UTF-8 `String`, replacing invalid sequences.
    ///
    /// Invalid UTF-8 sequences are replaced with the Unicode replacement character (U+FFFD).
    ///
    /// Requires the `alloc` feature.
    #[cfg(feature = "alloc")]
    #[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
    #[must_use]
    pub fn to_utf8_string_lossy(&self) -> String {
        String::from_utf8_lossy(&self.0).into_owned()
    }

    /// Consumes `self` and converts it to a UTF-8 `String`, replacing invalid sequences.
    ///
    /// Invalid UTF-8 sequences are replaced with the Unicode replacement character (U+FFFD).
    ///
    /// Requires the `alloc` feature.
    #[cfg(feature = "alloc")]
    #[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
    #[must_use]
    pub fn into_utf8_string_lossy(self) -> String {
        String::from_utf8_lossy(&self.0).into_owned()
    }
}

impl<const MAX: u32> TryFrom<Vec<u8>> for StringM<MAX> {
    type Error = Error;

    fn try_from(v: Vec<u8>) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(StringM(v))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

impl<const MAX: u32> From<StringM<MAX>> for Vec<u8> {
    fn from(v: StringM<MAX>) -> Self {
        v.0
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl<const MAX: u32> From<&StringM<MAX>> for Vec<u8> {
    fn from(v: &StringM<MAX>) -> Self {
        v.0.clone()
    }
}

impl<const MAX: u32> AsRef<Vec<u8>> for StringM<MAX> {
    fn as_ref(&self) -> &Vec<u8> {
        &self.0
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl<const MAX: u32> TryFrom<&Vec<u8>> for StringM<MAX> {
    type Error = Error;

    fn try_from(v: &Vec<u8>) -> Result<Self, Error> {
        v.as_slice().try_into()
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl<const MAX: u32> TryFrom<&[u8]> for StringM<MAX> {
    type Error = Error;

    fn try_from(v: &[u8]) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(StringM(v.to_vec()))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

impl<const MAX: u32> AsRef<[u8]> for StringM<MAX> {
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
impl<const N: usize, const MAX: u32> TryFrom<[u8; N]> for StringM<MAX> {
    type Error = Error;

    fn try_from(v: [u8; N]) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(StringM(v.to_vec()))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl<const N: usize, const MAX: u32> TryFrom<StringM<MAX>> for [u8; N] {
    type Error = StringM<MAX>;

    fn try_from(v: StringM<MAX>) -> core::result::Result<Self, Self::Error> {
        let s: [u8; N] = v.0.try_into().map_err(StringM::<MAX>)?;
        Ok(s)
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl<const N: usize, const MAX: u32> TryFrom<&[u8; N]> for StringM<MAX> {
    type Error = Error;

    fn try_from(v: &[u8; N]) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(StringM(v.to_vec()))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

#[cfg(not(feature = "alloc"))]
#[cfg_attr(docsrs, doc(cfg(not(feature = "alloc"))))]
impl<const N: usize, const MAX: u32> TryFrom<&'static [u8; N]> for StringM<MAX> {
    type Error = Error;

    fn try_from(v: &'static [u8; N]) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(StringM(v))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl<const MAX: u32> TryFrom<&String> for StringM<MAX> {
    type Error = Error;

    fn try_from(v: &String) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(StringM(v.as_bytes().to_vec()))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl<const MAX: u32> TryFrom<String> for StringM<MAX> {
    type Error = Error;

    fn try_from(v: String) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(StringM(v.into()))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl<const MAX: u32> TryFrom<StringM<MAX>> for String {
    type Error = Error;

    fn try_from(v: StringM<MAX>) -> Result<Self, Error> {
        Ok(String::from_utf8(v.0)?)
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl<const MAX: u32> TryFrom<&StringM<MAX>> for String {
    type Error = Error;

    fn try_from(v: &StringM<MAX>) -> Result<Self, Error> {
        Ok(core::str::from_utf8(v.as_ref())?.to_owned())
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl<const MAX: u32> TryFrom<&str> for StringM<MAX> {
    type Error = Error;

    fn try_from(v: &str) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(StringM(v.into()))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

#[cfg(not(feature = "alloc"))]
#[cfg_attr(docsrs, doc(cfg(not(feature = "alloc"))))]
impl<const MAX: u32> TryFrom<&'static str> for StringM<MAX> {
    type Error = Error;

    fn try_from(v: &'static str) -> Result<Self, Error> {
        let len: u32 = v.len().try_into().map_err(|_| Error::LengthExceedsMax)?;
        if len <= MAX {
            Ok(StringM(v.as_bytes()))
        } else {
            Err(Error::LengthExceedsMax)
        }
    }
}

impl<'a, const MAX: u32> TryFrom<&'a StringM<MAX>> for &'a str {
    type Error = Error;

    fn try_from(v: &'a StringM<MAX>) -> Result<Self, Error> {
        Ok(core::str::from_utf8(v.as_ref())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "alloc")]
    extern crate alloc;
    #[cfg(feature = "alloc")]
    use alloc::string::ToString;
    #[cfg(feature = "alloc")]
    use alloc::vec;
    #[cfg(feature = "alloc")]
    use alloc::vec::Vec;

    #[test]
    #[cfg(feature = "alloc")]
    fn test_to_utf8_string_lossy_valid_utf8() {
        // Valid UTF-8 string should pass through unchanged
        let s: StringM<100> = "Hello, World!".try_into().unwrap();
        assert_eq!(s.to_utf8_string_lossy(), "Hello, World!");
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_to_utf8_string_lossy_invalid_utf8() {
        // Invalid UTF-8 bytes should be replaced with replacement character
        let bytes = vec![0xFF, 0xFE, 0xFD];
        let s: StringM<100> = bytes.try_into().unwrap();
        let result = s.to_utf8_string_lossy();

        // Each invalid byte should be replaced with U+FFFD
        assert_eq!(result, "\u{FFFD}\u{FFFD}\u{FFFD}");
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_to_utf8_string_lossy_mixed() {
        // Mix of valid and invalid UTF-8
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"Hello");
        bytes.push(0xFF); // Invalid UTF-8
        bytes.extend_from_slice(b"World");

        let s: StringM<100> = bytes.try_into().unwrap();
        let result = s.to_utf8_string_lossy();

        assert_eq!(result, "Hello\u{FFFD}World");
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_into_utf8_string_lossy_valid_utf8() {
        // Valid UTF-8 string should pass through unchanged
        let s: StringM<100> = "Test String".try_into().unwrap();
        assert_eq!(s.into_utf8_string_lossy(), "Test String");
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_into_utf8_string_lossy_invalid_utf8() {
        // Invalid UTF-8 bytes should be replaced
        let bytes = vec![0xC0, 0xC1, 0xF5];
        let s: StringM<100> = bytes.try_into().unwrap();
        let result = s.into_utf8_string_lossy();

        // All bytes are invalid UTF-8 starts
        assert!(result.contains('\u{FFFD}'));
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_to_utf8_string_success() {
        // Valid UTF-8 should convert successfully
        let s: StringM<100> = "Valid UTF-8".try_into().unwrap();
        assert_eq!(s.to_utf8_string().unwrap(), "Valid UTF-8");
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_to_utf8_string_failure() {
        // Invalid UTF-8 should fail
        let bytes = vec![0xFF, 0xFE];
        let s: StringM<100> = bytes.try_into().unwrap();
        assert!(s.to_utf8_string().is_err());
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_into_utf8_string_success() {
        // Valid UTF-8 should convert successfully
        let s: StringM<100> = "Another test".try_into().unwrap();
        assert_eq!(s.into_utf8_string().unwrap(), "Another test");
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_into_utf8_string_failure() {
        // Invalid UTF-8 should fail
        let bytes = vec![0x80, 0x81];
        let s: StringM<100> = bytes.try_into().unwrap();
        assert!(s.into_utf8_string().is_err());
    }

    #[test]
    fn test_max_len_constraint() {
        // Test that length constraint is enforced
        let short: Result<StringM<5>, _> = "hello".try_into();
        assert!(short.is_ok());

        let too_long: Result<StringM<5>, _> = "hello world".try_into();
        assert!(too_long.is_err());
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_empty_string() {
        let s: StringM<100> = "".try_into().unwrap();
        assert_eq!(s.to_utf8_string_lossy(), "");
        assert_eq!(s.to_utf8_string().unwrap(), "");
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_unicode_characters() {
        // Test with various Unicode characters
        let s: StringM<100> = "Hello 世界 🌍".try_into().unwrap();
        assert_eq!(s.to_utf8_string_lossy(), "Hello 世界 🌍");
        assert_eq!(s.to_utf8_string().unwrap(), "Hello 世界 🌍");
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_fromstr_rejects_overlength() {
        let result = "abcd".parse::<StringM<3>>();
        assert!(matches!(result, Err(Error::LengthExceedsMax)));
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_fromstr_accepts_at_max() {
        let s = "abc".parse::<StringM<3>>().unwrap();
        assert_eq!(s.as_ref() as &[u8], b"abc");
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_fromstr_rejects_overlength_escaped() {
        let result = "\\x61\\x62\\x63\\x64".parse::<StringM<3>>();
        assert!(matches!(result, Err(Error::LengthExceedsMax)));
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_fromstr_roundtrip() {
        let original: StringM<10> = vec![0xFF, b'a', b'b'].try_into().unwrap();
        let displayed = original.to_string();
        let parsed: StringM<10> = displayed.parse().unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    #[cfg(all(feature = "serde", feature = "alloc"))]
    fn test_stringm_serde_serialize_rejects_overlimit() {
        let value = StringM::<1>(b"ab".to_vec());
        let err = serde_json::to_value(&value).unwrap_err();
        let msg = alloc::format!("{err}");
        assert!(msg.contains("max length exceeded"), "{msg}");
    }

    #[test]
    #[cfg(all(feature = "serde", feature = "alloc"))]
    fn test_stringm_serde_serialize_accepts_at_max() {
        let value = StringM::<2>(b"ab".to_vec());
        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(json, serde_json::json!("ab"));
    }
}
