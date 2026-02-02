mod common;
use common::*;

#[test]
fn infallible() {
    let val: Result<u32, core::convert::Infallible> = Ok(3);

    let expected = &hex!("0003");
    CXS::check_codec(&val, expected);

    let expected = &hex!("0000000000000003");
    TXS::check_codec(&val, expected);
}
