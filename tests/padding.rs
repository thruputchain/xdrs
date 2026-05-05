mod common;

use xdrs::{ReadError, txs};

#[test]
#[cfg(not(feature = "permissive"))]
fn txs_accepts_zero_padding_1_2_3_bytes() {
    let one_buf = hex!("00000001aa000000");
    let one: &[u8] = txs::decode(&one_buf).unwrap();
    assert_eq!(one, &[0xaa]);

    let two_buf = hex!("00000002aabb0000");
    let two: &[u8] = txs::decode(&two_buf).unwrap();
    assert_eq!(two, &[0xaa, 0xbb]);

    let three_buf = hex!("00000003aabbcc00");
    let three: &[u8] = txs::decode(&three_buf).unwrap();
    assert_eq!(three, &[0xaa, 0xbb, 0xcc]);
}

#[test]
#[cfg(not(feature = "permissive"))]
fn txs_rejects_non_zero_padding_1_2_3_bytes() {
    let e1 = txs::decode::<&[u8]>(&hex!("00000001aaff0000")).unwrap_err();
    assert!(matches!(e1, ReadError::NonZeroPadding));

    let e2 = txs::decode::<&[u8]>(&hex!("00000002aabb00ff")).unwrap_err();
    assert!(matches!(e2, ReadError::NonZeroPadding));

    let e3 = txs::decode::<&[u8]>(&hex!("00000003aabbccff")).unwrap_err();
    assert!(matches!(e3, ReadError::NonZeroPadding));
}

#[test]
#[cfg(feature = "permissive")]
fn txs_accepts_non_zero_padding_when_permissive() {
    let one_buf = hex!("00000001aaff0000");
    let one: &[u8] = txs::decode(&one_buf).unwrap();
    assert_eq!(one, &[0xaa]);

    let two_buf = hex!("00000002aabb00ff");
    let two: &[u8] = txs::decode(&two_buf).unwrap();
    assert_eq!(two, &[0xaa, 0xbb]);

    let three_buf = hex!("00000003aabbccff");
    let three: &[u8] = txs::decode(&three_buf).unwrap();
    assert_eq!(three, &[0xaa, 0xbb, 0xcc]);
}

// i16 and u16 are encoded as 4-byte wire integers. The upper two bytes are
// padding: in strict mode they must be zero (or sign-extension for i16),
// in permissive mode they are ignored and the value is truncated to 16 bits.

#[test]
#[cfg(not(feature = "permissive"))]
fn txs_rejects_u16_with_nonzero_padding() {
    // 00010001 = u32 65537, upper bytes 00 01 are non-zero padding
    let e = txs::decode::<u16>(&hex!("00010001")).unwrap_err();
    assert!(matches!(e, ReadError::IntegerOutOfRange));

    // 00FF0042 = u32 16711746, upper bytes 00 FF are non-zero padding
    let e = txs::decode::<u16>(&hex!("00FF0042")).unwrap_err();
    assert!(matches!(e, ReadError::IntegerOutOfRange));
}

#[test]
#[cfg(not(feature = "permissive"))]
fn txs_rejects_i16_with_nonzero_padding() {
    // 00010001 = i32 65537, canonical i16=1 has upper bytes 00 00
    let e = txs::decode::<i16>(&hex!("00010001")).unwrap_err();
    assert!(matches!(e, ReadError::IntegerOutOfRange));

    // FFFEFFFE = i32 -65538, canonical i16=-2 has upper bytes FF FF
    let e = txs::decode::<i16>(&hex!("FFFEFFFE")).unwrap_err();
    assert!(matches!(e, ReadError::IntegerOutOfRange));
}

#[test]
#[cfg(feature = "permissive")]
fn txs_accepts_u16_with_nonzero_padding_when_permissive() {
    // 00010001 = u32 65537, truncated to u16 = 1
    let v: u16 = txs::decode(&hex!("00010001")).unwrap();
    assert_eq!(v, 1);

    // 00FF0042 = u32 16711746, truncated to u16 = 0x0042 = 66
    let v: u16 = txs::decode(&hex!("00FF0042")).unwrap();
    assert_eq!(v, 66);

    // 00010000 = u32 65536, truncated to u16 = 0
    let v: u16 = txs::decode(&hex!("00010000")).unwrap();
    assert_eq!(v, 0);
}

#[test]
#[cfg(feature = "permissive")]
fn txs_accepts_i16_with_nonzero_padding_when_permissive() {
    // 00010001 = i32 65537, truncated to i16 = 1
    let v: i16 = txs::decode(&hex!("00010001")).unwrap();
    assert_eq!(v, 1);

    // FFFEFFFE = i32 -65538, truncated to i16 = -2
    let v: i16 = txs::decode(&hex!("FFFEFFFE")).unwrap();
    assert_eq!(v, -2);

    // 00008000 = i32 32768, truncated to i16 = -32768 (i16::MIN)
    let v: i16 = txs::decode(&hex!("00008000")).unwrap();
    assert_eq!(v, i16::MIN);
}

#[test]
fn txs_truncated_padding_still_reports_source_empty() {
    let e = txs::decode::<&[u8]>(&hex!("00000003aabbcc")).unwrap_err();
    assert!(matches!(e, ReadError::SourceEmpty { .. }));
}
