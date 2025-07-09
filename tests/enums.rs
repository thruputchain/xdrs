#![feature(maybe_uninit_array_assume_init)]
#![feature(maybe_uninit_slice)]

mod common;
use common::*;

#[test]
fn enum_bare() {
    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
    #[repr(i32)]
    enum Bare {
        Only,
        This,
    }

    let val = Bare::Only;
    let expected = &hex!("00");
    CXS::check_codec(&val, expected);

    let val = Bare::This;
    let expected = &hex!("01");
    CXS::check_codec(&val, expected);
}

#[test]
fn enum_foo() {
    const FOO_A: i32 = -1;

    #[allow(non_camel_case_types, dead_code)]
    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
    #[repr(i32)]
    enum Foo<'a> {
        A(&'a [u8]) = FOO_A,
        B { x: i32, y: u32 } = 7,
        C = 2,
    }

    let bytes = b"\r\n";
    let val = Foo::A(bytes);
    CXS::check_codec(&val, &hex!("3e020d0a"));

    let val = Foo::B { x: 0, y: 1 };
    CXS::check_codec(&val, &hex!("070001"));

    let val = Foo::C;
    CXS::check_codec(&val, &hex!("02"));
    TXS::check_codec(&val, &hex!("00000002"));
}

#[test]
fn result() {
    type R = Result<bool, u32>;

    let r: R = Ok(true);
    CXS::check_codec(&r, &hex!("0001"));
    TXS::check_codec(&r, &hex!("0000000000000001"));

    let r: R = Err(3);
    CXS::check_codec(&r, &hex!("0103"));
    TXS::check_codec(&r, &hex!("0000000100000003"));
}

// We don't support enums with some explicit variants and
// more than one implicit variant.
// This test should produce a compile error.
#[cfg(false)]
#[test]
fn bad_enum() {
    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
    #[repr(i32)]
    enum Animal {
        Ewe = 4,
        Eagle,
        Egret,
    }

    let val = Animal::Eagle;
    let expected = &hex!("05");
    CXS::check_codec(&val, expected);
}
