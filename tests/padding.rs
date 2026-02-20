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

#[test]
fn txs_truncated_padding_still_reports_source_empty() {
    let e = txs::decode::<&[u8]>(&hex!("00000003aabbcc")).unwrap_err();
    assert!(matches!(e, ReadError::SourceEmpty { .. }));
}
