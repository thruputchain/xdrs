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

#[derive(Clone, xdrs::ReadXDR, xdrs::WriteXDR)]
#[xdr(via = "u32")]
pub struct Converted(pub u32);

impl From<u32> for Converted {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<Converted> for u32 {
    fn from(value: Converted) -> Self {
        value.0
    }
}

#[derive(xdrs::ReadXDR, xdrs::WriteXDR, xdrs::XDR)]
#[repr(i32)]
pub enum Fieldless {
    First = 1,
    Second = 2,
}

// Keep handwritten code on the same source lines as derives: an exclusion
// must remove generated functions, not the whole source line or file.
#[derive(xdrs::WriteXDR)] pub struct Marker; pub fn handwritten() -> u32 { 7 }

#[test]
fn round_trip() {
    let bytes = xdrs::txs::encode(&Converted(11)).unwrap();
    let converted: Converted = xdrs::txs::decode(&bytes).unwrap();
    assert_eq!(converted.0, 11);
    let bytes = xdrs::txs::encode(&Fieldless::First).unwrap();
    let fieldless: Fieldless = xdrs::txs::decode(&bytes).unwrap();
    assert!(matches!(fieldless, Fieldless::First));
    let _ = <Fieldless as xdrs::xdr_repr::HasTypeDef>::type_def();
    let _ = <Record as xdrs::xdr_repr::HasTypeDef>::type_def();
    let mut writer = xdrs::cxs::ConstWriter::<16>::new();
    Record { value: 7 }.const_write_cxs(&mut writer);
    let bytes = xdrs::txs::encode(&Record { value: handwritten() }).unwrap();
    let value: Record = xdrs::txs::decode(&bytes).unwrap();
    assert_eq!(value.value, 7);
}
