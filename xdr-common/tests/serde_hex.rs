#![cfg(feature = "serde")]

use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Opaque<const N: usize>(#[serde(with = "xdr_common::serde_hex")] [u8; N]);

#[test]
fn serializes_lowercase_hex_with_leading_zeroes() {
    let value = Opaque([0x00, 0x01, 0x0a, 0x10, 0xab, 0xcd, 0xef, 0xff]);
    assert_eq!(
        serde_json::to_value(value).unwrap(),
        json!("00010a10abcdefff")
    );
}

#[test]
fn accepts_lowercase_uppercase_and_mixed_case_hex() {
    for hex in ["0001abcdef", "0001ABCDEF", "0001aBcDeF"] {
        let value: Opaque<5> = serde_json::from_value(json!(hex)).unwrap();
        assert_eq!(value, Opaque([0x00, 0x01, 0xab, 0xcd, 0xef]));
    }
}

#[test]
fn supports_borrowed_owned_and_escaped_strings() {
    let expected = Opaque([0xab, 0xcd]);
    assert_eq!(
        serde_json::from_str::<Opaque<2>>(r#""abcd""#).unwrap(),
        expected
    );
    assert_eq!(
        serde_json::from_value::<Opaque<2>>(json!("abcd")).unwrap(),
        expected
    );
    assert_eq!(
        serde_json::from_str::<Opaque<2>>(r#""\u0061bcd""#).unwrap(),
        expected
    );
    assert_eq!(
        serde_json::from_reader::<_, Opaque<2>>(r#""abcd""#.as_bytes()).unwrap(),
        expected
    );
}

#[test]
fn empty_array_requires_an_empty_string() {
    assert_eq!(serde_json::to_value(Opaque([])).unwrap(), json!(""));
    assert_eq!(
        serde_json::from_value::<Opaque<0>>(json!("")).unwrap(),
        Opaque([])
    );
    for invalid in ["0", "00", "é"] {
        assert_invalid::<0>(invalid);
    }
}

#[test]
fn roundtrips_arrays_larger_than_32_bytes() {
    let value = Opaque([0xab; 64]);
    assert_eq!(
        serde_json::to_value(&value).unwrap(),
        json!("ab".repeat(64))
    );
    assert_eq!(
        serde_json::from_value::<Opaque<64>>(json!("AB".repeat(64))).unwrap(),
        value
    );

    let all_bytes = Opaque(core::array::from_fn::<_, 256, _>(|i| i as u8));
    let encoded = serde_json::to_string(&all_bytes).unwrap();
    assert_eq!(encoded.len(), 256 * 2 + 2);
    assert_eq!(
        serde_json::from_str::<Opaque<256>>(&encoded).unwrap(),
        all_bytes
    );
}

#[test]
fn rejects_short_long_and_odd_lengths() {
    for invalid in ["", "0", "00", "000", "00000", "000000"] {
        assert_invalid::<2>(invalid);
    }
}

#[test]
fn rejects_non_hex_ascii_in_either_digit() {
    for invalid in ["g0", "0g", "G0", "0G", " 0", "0 ", "+1", "-1", "0x"] {
        assert_invalid::<1>(invalid);
    }
    assert_invalid::<2>("0x00");
    assert_invalid::<2>("00\n0");
}

#[test]
fn rejects_non_ascii_at_pair_boundaries_without_panicking() {
    // All of these have the expected four-byte length. Slicing str into
    // two-byte pairs used to panic on "€0", "0€", "aé0", and "😀".
    // "éé" exercises non-ASCII input whose pair boundaries are valid UTF-8.
    for invalid in ["€0", "0€", "aé0", "😀", "éé"] {
        assert_eq!(invalid.len(), 4);
        assert_invalid::<2>(invalid);
    }

    // Also reach the bad boundary after successfully decoding ASCII pairs.
    for invalid in ["00€0", "00aé0", "00😀"] {
        assert_eq!(invalid.len(), 6);
        assert_invalid::<3>(invalid);
    }
}

#[test]
fn rejects_escaped_non_ascii_without_panicking() {
    // JSON escapes take Serde's transient-string path after unescaping.
    for invalid in [r#""\u20ac0""#, r#""a\u00e90""#, r#""\ud83d\ude00""#] {
        assert!(serde_json::from_str::<Opaque<2>>(invalid).is_err());
    }
}

#[test]
fn rejects_non_string_values() {
    for invalid in [json!(null), json!(1), json!(true), json!([0, 1]), json!({})] {
        assert!(serde_json::from_value::<Opaque<2>>(invalid).is_err());
    }
}

fn assert_invalid<const N: usize>(hex: &str) {
    let encoded = serde_json::to_string(hex).unwrap();
    assert!(
        serde_json::from_str::<Opaque<N>>(&encoded).is_err(),
        "accepted {hex:?} as {N} bytes"
    );
    assert!(
        serde_json::from_value::<Opaque<N>>(json!(hex)).is_err(),
        "accepted owned {hex:?} as {N} bytes"
    );
}
