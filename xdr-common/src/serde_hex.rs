//! Serde adapter for fixed-size byte arrays encoded as hexadecimal strings.
//!
//! Use `#[serde(with = "xdr_common::serde_hex")]` on an `[u8; N]` field
//! with the `serde` feature enabled. This module is also re-exported as
//! `xdrs::serde_hex` when that crate's `serde` feature is enabled.
//!
//! Serialization always emits a lowercase hex string, including for binary
//! serializers. Deserialization accepts upper- or lowercase ASCII hex and
//! requires exactly two digits per byte. Prefixes, whitespace, and non-ASCII
//! characters are rejected. An empty array is represented by an empty string.
//!
//! ```
//! #[derive(serde::Serialize, serde::Deserialize)]
//! struct Hash {
//!     #[serde(with = "xdr_common::serde_hex")]
//!     bytes: [u8; 32],
//! }
//! ```

use core::fmt;

use serde::{Deserializer, Serializer, de};

use crate::bytesm::hex_digit;

/// Serialize a fixed-size byte array as a lowercase hexadecimal string.
pub fn serialize<S, const N: usize>(bytes: &[u8; N], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.collect_str(&HexDisplay(bytes))
}

struct HexDisplay<'a>(&'a [u8]);

impl fmt::Display for HexDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Deserialize a hexadecimal string into a fixed-size byte array.
///
/// The decoded bytes are written directly into the array without an
/// intermediate vector. Invalid hex or an incorrect length returns a
/// deserializer error.
pub fn deserialize<'de, D, const N: usize>(deserializer: D) -> Result<[u8; N], D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_str(HexVisitor::<N>)
}

struct HexVisitor<const N: usize>;

impl<'de, const N: usize> de::Visitor<'de> for HexVisitor<N> {
    type Value = [u8; N];

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a hexadecimal string encoding exactly {N} bytes")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        // Divide the input length rather than multiplying N, which could
        // overflow for a sufficiently large array.
        if !value.len().is_multiple_of(2) || value.len() / 2 != N {
            return Err(E::invalid_length(value.len(), &self));
        }

        let mut bytes = [0; N];
        // Chunk bytes, never str: a non-ASCII character can cross a pair
        // boundary even when the total byte length is correct.
        let (pairs, _) = value.as_bytes().as_chunks::<2>();
        for (byte, pair) in bytes.iter_mut().zip(pairs) {
            *byte = (hex_digit(pair[0]).map_err(E::custom)? << 4)
                | hex_digit(pair[1]).map_err(E::custom)?;
        }
        Ok(bytes)
    }
}
