#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

use xdrs::xdr_repr::XDR;

#[derive(xdrs::ReadXDR, xdrs::WriteXDR, xdrs::XDR, xdrs::ConstWriteXDR)]
pub struct Record {
    pub value: u32,
}

#[derive(xdrs::ReadXDR, xdrs::WriteXDR, xdrs::XDR, xdrs::ConstWriteXDR)]
#[repr(i32)]
pub enum Choice {
    Value(u32) = 1,
    Empty = 2,
}

// Keep handwritten code on the same source lines as derives: an exclusion
// must remove generated functions, not the whole source line or file.
#[derive(xdrs::WriteXDR)] pub struct Marker; pub fn handwritten() -> u32 { 7 }

#[test]
fn round_trip() {
    let _ = <Record as xdrs::xdr_repr::HasTypeDef>::type_def();
    let mut writer = xdrs::cxs::ConstWriter::<16>::new();
    Record { value: 7 }.const_write_cxs(&mut writer);
    let bytes = xdrs::txs::encode(&Record { value: handwritten() }).unwrap();
    let value: Record = xdrs::txs::decode(&bytes).unwrap();
    assert_eq!(value.value, 7);
}
