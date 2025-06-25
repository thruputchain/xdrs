#![feature(maybe_uninit_array_assume_init)]

use ::cxs::*;

mod common;
use common::*;

#[test]
fn enum_bare() {
    #[derive(Debug, PartialEq, Eq, WriteXDR)]
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

    let val = Bare::This;
    let expected = &hex!("01");
    check_encode(&val, expected);
}

#[test]
fn enum_foo() {
    #[derive(Debug, PartialEq, Eq, WriteXDR)]
    #[repr(i32)]
    enum Foo<'a> {
        A(Bytes<'a>),
        B { x: i32, y: i32 },
    }

    impl<'a> XDRDiscriminant for Foo<'a> {
        fn discriminant(&self) -> i32 {
            unsafe { *<*const _>::from(self).cast::<i32>() }
        }
    }

    let val = Foo::B { x: 0, y: 1 };
    let expected = &hex!("010001");
    check_encode(&val, expected);

    let bytes = b"\r\n";
    let val = Foo::A(Bytes::from(bytes));
    let expected = &hex!("00020d0a");
    check_encode(&val, expected);
}
