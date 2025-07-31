// use xdr_repr::xdr::*;
use xdr_repr::ty::*;
use xdr_repr::{check_format, assert_formatted, lines};
use xdr_repr::format::*;
use xdr_repr::construct::*;
use xdr_derive::XDR;

#[derive(XDR)]
#[repr(i32)]
pub enum E {
    Alpha = 1,
    Beta = 2,
    Omega = -1,
}

check_format! { derive_enum
    E::def()
    =>
        "enum E {"
        "	Alpha = 1;"
        "	Beta = 2;"
        "	Omega = -1;"
        "};"
}

#[derive(XDR)]
#[repr(u32)]
pub enum EnumBadRepr {
    Alpha = 1,
}

// TODO: This test fails, as the bad repr is not yet inspected
check_format! { #[should_panic] derive_enum_bad_repr
    EnumBadRepr::def()
    =>
        "enum EnumBadRepr {"
        "	Alpha = 1;"
        "};"
}

#[derive(XDR)]
#[repr(i32)]
pub enum Data {
    Alpha(u32) = 1,
    Beta = 2,
    Omega = -1,
}

// TODO: This test fails, as we don't yet produce unions
check_format! { derive_enum_data
    Data::spec()
    =>
        "enum DataType {"
        "	Alpha = 1;"
        "	Beta = 2;"
        "	Omega = -1;"
        "};"
        ""
        "union Data switch (DataType ty) {"
        "	Alpha = 1;"
        "	Beta = 2;"
        "	Omega = -1;"
        "};"
        ""
}
