mod common;
use common::*;

#[cfg(feature = "alloc")]
#[test]
fn vecm_empty() {
    use xdrs::*;

    let val: VecM<u32> = VecM::default();

    CXS::check_codec(&val, &hex!("00"));
    TXS::check_codec(&val, &hex!("00000000"));
}

#[cfg(feature = "alloc")]
#[test]
fn vecm_full() {
    use xdrs::*;

    let val: VecM<u32> = VecM(vec![1]);

    CXS::check_codec(&val, &hex!("0101"));
    TXS::check_codec(&val, &hex!("0000000100000001"));
}

#[cfg(feature = "alloc")]
#[test]
fn stringm_full() {
    use xdrs::*;

    let val: StringM<10> = StringM("hello".as_bytes().into());

    CXS::check_codec(&val, &hex!("0568656c6c6f"));
    TXS::check_codec(&val, &hex!("0000000568656c6c6f000000"));
}

#[cfg(feature = "alloc")]
#[test]
fn vec_in_struct() {
    use xdrs::*;

    #[derive(Debug, PartialEq, Eq, xdrs::ReadXDR, xdrs::WriteXDR)]
    #[repr(C)]
    struct Two {
        pub a: Vec<u8>,
        pub b: bool,
    }

    let val = Two {
        a: Vec::default(),
        b: true,
    };

    CXS::check_decode(&val, &hex!("0001"));
    TXS::check_codec(&val, &hex!("0000000000000001"));
}

#[test]
fn slice_in_struct() {
    let arr: [u8; 0] = [];
    let buf = &arr[..];

    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
    #[repr(C)]
    struct Two<'a> {
        pub a: &'a [u8],
        pub b: bool,
    }

    let val = Two { a: buf, b: true };

    CXS::check_codec(&val, &hex!("0001"));
    TXS::check_codec(&val, &hex!("0000000000000001"));
}

#[cfg(feature = "alloc")]
#[test]
fn vec_in_tuple() {
    use xdrs::*;

    let val = (Vec::<u32>::default(), true);

    CXS::check_encode(&val, &hex!("0001"));
    CXS::check_decode(&val, &hex!("0001"));
}

#[cfg(feature = "alloc")]
#[test]
fn recursive_struct() {
    use xdrs::*;

    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
    #[repr(C)]
    struct Recursive {
        pub elem: BytesM,
        pub nextvec: VecM<Recursive>,
    }

    let val = Recursive {
        elem: BytesM("hello".as_bytes().into()),
        nextvec: VecM::default(),
    };

    CXS::check_codec(&val, &hex!("0568656c6c6f00"));
    TXS::check_codec(&val, &hex!("0000000568656c6c6f00000000000000"));
}

#[cfg(feature = "alloc")]
#[test]
fn recursive_struct_box() {
    use xdrs::*;

    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
    #[repr(C)]
    struct Recursive {
        pub elem: BytesM,
        pub next: Option<Box<Recursive>>,
    }

    let val = Recursive {
        elem: BytesM("hello".as_bytes().into()),
        next: None,
    };

    CXS::check_codec(&val, &hex!("0568656c6c6f00"));
    TXS::check_codec(&val, &hex!("0000000568656c6c6f00000000000000"));
}

#[test]
fn option_test() {
    let x: Option<i32> = None;
    let y: Option<i32> = Some(1);

    CXS::check_codec(&x, &hex!("00"));
    TXS::check_codec(&x, &hex!("00000000"));

    CXS::check_codec(&y, &hex!("0101"));
    TXS::check_codec(&y, &hex!("0000000100000001"));
}

#[cfg(feature = "alloc")]
#[test]
fn option_box_test() {
    let x: Option<Box<i32>> = None;
    let y: Option<Box<i32>> = Some(Box::new(1));

    CXS::check_codec(&x, &hex!("00"));
    TXS::check_codec(&x, &hex!("00000000"));

    CXS::check_codec(&y, &hex!("0101"));
    TXS::check_codec(&y, &hex!("0000000100000001"));
}

// --- Fix 5: bounded-length types reject over-length input at the reader level ---

#[cfg(feature = "alloc")]
#[test]
fn bytesm_rejects_overlength() {
    use xdrs::*;

    // BytesM<3>: encode a 4-byte opaque, which exceeds MAX=3
    // TXS: length prefix 4 (00000004) + 4 data bytes + 0 padding
    let buf = hex!("000000040102030400000000");
    let err = xdrs::txs::decode::<BytesM<3>>(&buf).unwrap_err();
    assert!(matches!(err, ReadError::MaxLengthExceeded));
}

#[cfg(feature = "alloc")]
#[test]
fn bytesm_accepts_at_max() {
    use xdrs::*;

    // BytesM<3>: encode a 3-byte opaque, exactly at MAX=3
    // TXS: length prefix 3 (00000003) + 3 data bytes + 1 padding
    let buf = hex!("0000000301020300");
    let val: BytesM<3> = xdrs::txs::decode(&buf).unwrap();
    assert_eq!(&*val.0, &[1, 2, 3]);
}

#[cfg(feature = "alloc")]
#[test]
fn stringm_rejects_overlength() {
    use xdrs::*;

    // StringM<4>: encode a 5-byte string, which exceeds MAX=4
    // TXS: length prefix 5 (00000005) + "hello" + 3 padding
    let buf = hex!("0000000568656c6c6f000000");
    let err = xdrs::txs::decode::<StringM<4>>(&buf).unwrap_err();
    assert!(matches!(err, ReadError::MaxLengthExceeded));
}

#[cfg(feature = "alloc")]
#[test]
fn stringm_accepts_at_max() {
    use xdrs::*;

    // StringM<5>: encode a 5-byte string, exactly at MAX=5
    // TXS: length prefix 5 (00000005) + "hello" + 3 padding
    let buf = hex!("0000000568656c6c6f000000");
    let val: StringM<5> = xdrs::txs::decode(&buf).unwrap();
    assert_eq!(&*val.0, b"hello");
}

#[cfg(feature = "alloc")]
#[test]
fn vecm_u8_rejects_overlength() {
    use xdrs::*;

    // VecM<u8, 2>: encode a 3-byte opaque, which exceeds MAX=2
    // TXS: length prefix 3 (00000003) + 3 data bytes + 1 padding
    let buf = hex!("0000000301020300");
    let err = xdrs::txs::decode::<VecM<u8, 2>>(&buf).unwrap_err();
    assert!(matches!(err, ReadError::MaxLengthExceeded));
}

#[cfg(feature = "alloc")]
#[test]
fn vecm_u8_accepts_at_max() {
    use xdrs::*;

    // VecM<u8, 3>: encode a 3-byte opaque, exactly at MAX=3
    let buf = hex!("0000000301020300");
    let val: VecM<u8, 3> = xdrs::txs::decode(&buf).unwrap();
    assert_eq!(&*val.0, &[1, 2, 3]);
}

#[cfg(feature = "alloc")]
#[test]
fn vecm_generic_rejects_overlength() {
    use xdrs::*;

    // VecM<u32, 2>: encode a vec of 3 u32s, which exceeds MAX=2.
    // The length prefix claims 3 elements. Before fix 5 this would
    // allocate space for 3 elements before checking the bound;
    // with a malicious length prefix (e.g. 0x7FFFFFFF) it would OOM.
    // TXS: length prefix 3 + three u32s (1, 2, 3)
    let buf = hex!("00000003000000010000000200000003");
    let err = xdrs::txs::decode::<VecM<u32, 2>>(&buf).unwrap_err();
    assert!(matches!(err, ReadError::MaxLengthExceeded));
}

#[cfg(feature = "alloc")]
#[test]
fn vecm_generic_rejects_huge_length_without_allocating() {
    use xdrs::*;

    // VecM<u32, 2>: craft a payload whose length prefix is enormous
    // (0x7FFF_FFFF = ~2 billion elements). Before fix 5, Vec<u32>::read_xdr
    // would try to Vec::with_capacity(2_147_483_647) and OOM.
    // After fix 5, VecM checks len > MAX before allocating.
    // We only need the 4-byte length prefix; the reader will reject
    // before trying to read any elements.
    let buf = hex!("7FFFFFFF");
    let err = xdrs::txs::decode::<VecM<u32, 2>>(&buf).unwrap_err();
    assert!(matches!(err, ReadError::MaxLengthExceeded));
}

#[cfg(feature = "alloc")]
#[test]
fn vecm_generic_accepts_at_max() {
    use xdrs::*;

    // VecM<u32, 3>: encode a vec of 3 u32s, exactly at MAX=3
    let buf = hex!("00000003000000010000000200000003");
    let val: VecM<u32, 3> = xdrs::txs::decode(&buf).unwrap();
    assert_eq!(&*val.0, &[1u32, 2, 3]);
}

#[cfg(feature = "alloc")]
#[test]
fn bare_vec_rejects_huge_length_without_oom() {
    use xdrs::*;

    // Vec<u32> with a huge length prefix claiming 0x7FFF_FFFF elements,
    // followed by nothing. Before fix: Vec::with_capacity(0x7FFF_FFFF) tries
    // to allocate ~8GB. After fix: capacity is capped, and the read fails
    // gracefully with SourceEmpty.
    let buf = hex!("7FFFFFFF");
    let err = xdrs::txs::decode::<Vec<u32>>(&buf).unwrap_err();
    assert!(matches!(err, ReadError::SourceEmpty { .. }));
}

#[cfg(feature = "alloc")]
#[test]
fn vec_does_not_overconsume_from_wire() {
    use xdrs::*;

    // Encode: (Vec<u32> of [1, 2, 3], then a u32 = 99)
    // When decoding, the Vec must consume exactly `len` elements and not eat
    // into subsequent data.
    //
    // with_capacity() might or might not overallocate, so I do not have a reliable
    // test case to trigger the prior bug.
    let input: (Vec<u32>, u32) = (vec![1, 2, 3], 99);
    let encoded = xdrs::txs::encode(&input).unwrap();
    let decoded: (VecM<u32, 4>, u32) = xdrs::txs::decode(&encoded).unwrap();
    let expected = VecM(vec![1u32, 2, 3]);
    assert_eq!(decoded.0, expected);
    assert_eq!(decoded.1, 99);
}
