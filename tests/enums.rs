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
    check_encode(&val, expected);
    check_decode(&val, expected);

    let val = Bare::This;
    let expected = &hex!("01");
    check_encode(&val, expected);
    check_decode(&val, expected);
}

#[test]
fn enum_foo() {
    const FOO_A: i32 = -1;

    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
    #[repr(i32)]
    enum Foo<'a> {
        A(&'a [u8]) = FOO_A,
        B { x: i32, y: u32 } = 7,
        C = 2,
        D(bool), // note lack of explicit discriminant
    }

    let bytes = b"\r\n";
    let val = Foo::A(bytes);
    let expected = &hex!("3e020d0a");
    check_codec(&val, expected);

    let val = Foo::B { x: 0, y: 1 };
    let expected = &hex!("070001");
    check_codec(&val, expected);

    let val = Foo::C;
    let expected = &hex!("02");
    check_codec(&val, expected);

    let val = Foo::D(true);
    let expected = &hex!("0301");
    check_codec(&val, expected);
    let expected = &hex!("0b01");
    check_decode(&val, expected);
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
    check_encode(&val, expected);
    check_decode(&val, expected);
}
