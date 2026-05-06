#![cfg(feature = "alloc")]

mod common;
use common::*;

use xdrs::{BytesM, StringM, VecM, WriteError};

fn assert_encode_rejects<T: xdrs::WriteXDR>(label: &str, value: &T) {
    #[cfg(feature = "alloc")]
    {
        let txs_result = TXS::encode(value);
        assert!(
            matches!(txs_result, Err(WriteError::InvalidArrayLength)),
            "{label}: TXS encode should reject over-MAX value, got {txs_result:?}"
        );
        let cxs_result = CXS::encode(value);
        assert!(
            matches!(cxs_result, Err(WriteError::InvalidArrayLength)),
            "{label}: CXS encode should reject over-MAX value, got {cxs_result:?}"
        );
    }

    let mut buf = [core::mem::MaybeUninit::zeroed(); 256];
    let txs_result = TXS::encode_into(value, &mut buf);
    assert!(
        matches!(txs_result, Err(WriteError::InvalidArrayLength)),
        "{label}: TXS encode_into should reject over-MAX value, got {txs_result:?}"
    );
    let cxs_result = CXS::encode_into(value, &mut buf);
    assert!(
        matches!(cxs_result, Err(WriteError::InvalidArrayLength)),
        "{label}: CXS encode_into should reject over-MAX value, got {cxs_result:?}"
    );
}

#[test]
fn bytesm_rejects_overlong_write() {
    assert_encode_rejects("BytesM<2> len=3", &BytesM::<2>(vec![1, 2, 3]));
    assert_encode_rejects("BytesM<0> len=1", &BytesM::<0>(vec![1]));
}

#[test]
fn bytesm_accepts_at_max() {
    let val = BytesM::<3>(vec![1, 2, 3]);
    CXS::check_encode(&val, &hex!("03010203"));
    TXS::check_encode(&val, &hex!("0000000301020300"));
}

#[test]
fn stringm_rejects_overlong_write() {
    assert_encode_rejects("StringM<2> len=3", &StringM::<2>(b"abc".to_vec()));
    assert_encode_rejects("StringM<0> len=1", &StringM::<0>(b"x".to_vec()));
}

#[test]
fn stringm_accepts_at_max() {
    let val = StringM::<3>(b"abc".to_vec());
    CXS::check_encode(&val, &hex!("03616263"));
    TXS::check_encode(&val, &hex!("0000000361626300"));
}

#[test]
fn vecm_u8_rejects_overlong_write() {
    assert_encode_rejects("VecM<u8,2> len=3", &VecM::<u8, 2>(vec![1, 2, 3]));
    assert_encode_rejects("VecM<u8,0> len=1", &VecM::<u8, 0>(vec![1]));
}

#[test]
fn vecm_u8_accepts_at_max() {
    let val = VecM::<u8, 3>(vec![1, 2, 3]);
    CXS::check_encode(&val, &hex!("03010203"));
    TXS::check_encode(&val, &hex!("0000000301020300"));
}

#[test]
fn vecm_u32_rejects_overlong_write() {
    assert_encode_rejects("VecM<u32,2> len=3", &VecM::<u32, 2>(vec![1, 2, 3]));
    assert_encode_rejects("VecM<u32,0> len=1", &VecM::<u32, 0>(vec![1]));
}

#[test]
fn vecm_u32_accepts_at_max() {
    let val = VecM::<u32, 2>(vec![1, 2]);
    CXS::check_encode(&val, &hex!("020102"));
    TXS::check_encode(&val, &hex!("000000020000000100000002"));
}

#[test]
fn bounded_empty_always_accepted() {
    let bytes = BytesM::<0>(vec![]);
    CXS::check_encode(&bytes, &hex!("00"));
    TXS::check_encode(&bytes, &hex!("00000000"));

    let string = StringM::<0>(vec![]);
    CXS::check_encode(&string, &hex!("00"));
    TXS::check_encode(&string, &hex!("00000000"));

    let vec_u8 = VecM::<u8, 0>(vec![]);
    CXS::check_encode(&vec_u8, &hex!("00"));
    TXS::check_encode(&vec_u8, &hex!("00000000"));

    let vec_u32 = VecM::<u32, 0>(vec![]);
    CXS::check_encode(&vec_u32, &hex!("00"));
    TXS::check_encode(&vec_u32, &hex!("00000000"));
}
