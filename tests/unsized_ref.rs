// tests/unsized_ref.rs
//
// `&T` must implement WriteXDR for unsized `T`, so that a `#[derive(WriteXDR)]`
// struct can hold a `&'a T` field under a `T: WriteXDR + ?Sized` bound.

mod common;
use common::*;

// --- The gap: a generic `&'a T` field with a `?Sized` bound ---

#[derive(Debug, PartialEq, Eq, xdrs::WriteXDR)]
struct Borrowed<'a, T: xdrs::WriteXDR + ?Sized> {
    tag: u32,
    inner: &'a T,
}

// The same shape, spelled with a concrete sized type, as the reference encoding.
#[derive(Debug, PartialEq, Eq, xdrs::WriteXDR)]
struct BorrowedConcrete<'a> {
    tag: u32,
    inner: &'a str,
}

#[test]
fn generic_unsized_str_field_matches_sized_spelling() {
    let generic: Borrowed<'_, str> = Borrowed {
        tag: 7,
        inner: "hi",
    };
    let concrete = BorrowedConcrete {
        tag: 7,
        inner: "hi",
    };

    assert_eq!(
        xdrs::cxs::encode(&generic).unwrap(),
        xdrs::cxs::encode(&concrete).unwrap()
    );
    assert_eq!(
        xdrs::txs::encode(&generic).unwrap(),
        xdrs::txs::encode(&concrete).unwrap()
    );
}

#[test]
fn generic_unsized_bytes_field_encodes_as_opaque() {
    let val: Borrowed<'_, [u8]> = Borrowed {
        tag: 7,
        inner: &[0xaa, 0xbb, 0xcc],
    };

    CXS::check_encode(&val, &hex!("0703aabbcc"));
    TXS::check_encode(&val, &hex!("0000000700000003aabbcc00"));
}

#[test]
fn generic_unsized_slice_field_encodes_as_array() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, xdrs::WriteXDR)]
    struct Leaf {
        code: u32,
    }

    let items = [Leaf { code: 7 }, Leaf { code: 42 }];
    let val: Borrowed<'_, [Leaf]> = Borrowed {
        tag: 1,
        inner: &items,
    };

    CXS::check_encode(&val, &hex!("0102072a"));
    TXS::check_encode(&val, &hex!("0000000100000002000000070000002a"));
}

// --- Byte-identity pins for the reference impls that the blanket impl subsumes ---

#[test]
fn top_level_str_ref_encoding() {
    let s: &str = "hi";

    CXS::check_encode(&s, &hex!("026869"));
    TXS::check_encode(&s, &hex!("0000000268690000"));
}

#[test]
fn top_level_bytes_ref_encoding() {
    let b: &[u8] = &[0xaa, 0xbb, 0xcc];

    CXS::check_encode(&b, &hex!("03aabbcc"));
    TXS::check_encode(&b, &hex!("00000003aabbcc00"));
}
