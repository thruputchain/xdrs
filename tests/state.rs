mod common;
use common::*;
use xdrs::{ReadXDR, ReadXDRState, ReadResult, read_xdr_state, WriteXDR, ReaderState};

#[derive(Debug, PartialEq, Eq, WriteXDR, ReadXDR)]
struct Foo {
    bytes: [u8; 3],
    blerg: i32,
    #[xdrs(skip)]
    path: Vec<u32>,
}

impl<'r> ReadXDRState<'r> for Foo {
    fn read_xdr_state<R: ReaderState<'r>>(r: &mut R) -> ReadResult<Self> {
        let bytes = read_xdr_state(r)?;
        let blerg = read_xdr_state(r)?;
        let path = r.path().into();

        Ok(Foo { bytes, blerg, path })
    }
}

#[test]
fn seven() {
    let val: [Foo; 1] = [Foo {
        bytes: [0xAA, 0xBB, 0xCC],
        blerg: 7,
        path: vec![0],
    }];
    let expected = hex!("aabbcc07");

    CXS::check_codec_state(&val, &expected);
}
