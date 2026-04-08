mod common;
use common::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, xdrs::WriteXDR)]
struct Leaf {
    code: u32,
}

#[test]
fn borrowed_bytes_field_keeps_opaque_encoding() {
    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
    struct BorrowedBytes<'a> {
        tag: u32,
        bytes: &'a [u8],
    }

    let bytes = [0xaa, 0xbb, 0xcc];
    let val = BorrowedBytes {
        tag: 7,
        bytes: &bytes,
    };

    CXS::check_codec(&val, &hex!("0703aabbcc"));
    TXS::check_codec(&val, &hex!("0000000700000003aabbcc00"));
}

#[test]
fn top_level_borrowed_leaf_slice_already_encodes() {
    let items = [Leaf { code: 7 }, Leaf { code: 42 }];
    let slice: &[Leaf] = &items;

    CXS::check_encode(slice, &hex!("02072a"));
    TXS::check_encode(slice, &hex!("00000002000000070000002a"));
}
