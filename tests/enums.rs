#![feature(maybe_uninit_array_assume_init)]

use ::cxs::*;

mod common;
use common::*;

#[test]
fn enum_bare() {
    #[derive(Debug, PartialEq, Eq, WriteXDR, ReadXDR)]
    #[repr(i32)]
    enum Bare {
        Only,
        This,
    }

    impl XDRDiscriminant for Bare {
        fn discriminant(&self) -> i32 {
            unsafe { *<*const _>::from(self).cast::<i32>() }
        }
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

    #[derive(Debug, PartialEq, Eq, WriteXDR, ReadXDR)]
    #[repr(i32)]
    enum Foo<'a> {
        A(Bytes<'a>) = FOO_A,
        B { x: i32, y: u32 } = 7,
        C = 2,
    }

    impl<'a> XDRDiscriminant for Foo<'a> {
        fn discriminant(&self) -> i32 {
            unsafe { *<*const _>::from(self).cast::<i32>() }
        }
    }

    let bytes = b"\r\n";
    let val = Foo::A(Bytes::from(bytes));
    let expected = &hex!("3e020d0a");
    check_codec(&val, expected);

    let val = Foo::B { x: 0, y: 1 };
    let expected = &hex!("070001");
    check_codec(&val, expected);

    let val = Foo::C;
    let expected = &hex!("02");
    check_codec(&val, expected);
}

// We don't support enums with a mix of implicit and explicit discriminators.
// This test should produce a compile error.
#[cfg(false)]
#[test]
fn bad_enum() {
    #[derive(Debug, PartialEq, Eq, WriteXDR, ReadXDR)]
    #[repr(i32)]
    enum Animal {
        Ewe = 4,
        Eagle,
    }

    impl<'a> XDRDiscriminant for Animal {
        fn discriminant(&self) -> i32 {
            unsafe { *<*const _>::from(self).cast::<i32>() }
        }
    }

    let val = Animal::Eagle;
    let expected = &hex!("05");
    check_encode(&val, expected);
    check_decode(&val, expected);
}
