mod common;
use common::*;
use core::ops::Deref;
use xdrs::{ReadResult, ReadXDR, Reader, WriteXDR, read_xdr};

#[test]
fn struct_foo() {
    #[derive(Debug, PartialEq, Eq, WriteXDR, ReadXDR)]
    struct Foo {
        bytes: [u8; 3],
        blerg: i32,
    }

    // Given: a value and its expected encoding
    let val: Foo = Foo {
        bytes: [0xAA, 0xBB, 0xCC],
        blerg: 7,
    };

    let expected = &hex!("aabbcc07");
    CXS::check_codec(&val, expected);

    let expected = &hex!("aabbcc0000000007");
    TXS::check_codec(&val, expected);
}

#[test]
fn struct_boo() {
    #[derive(Debug, PartialEq, Eq, WriteXDR, ReadXDR)]
    struct Boo<'a> {
        blerg: i32,
        bytes: &'a [u8; 9],
    }

    let bytes = [7u8; 9];
    let val = Boo {
        blerg: -5,
        bytes: &bytes,
    };

    let expected = &hex!("3a070707070707070707");
    CXS::check_codec(&val, expected);

    let expected = &hex!("fffffffb070707070707070707000000");
    TXS::check_codec(&val, expected);
}

#[test]
fn struct_unit() {
    #[derive(Debug, PartialEq, Eq, WriteXDR, ReadXDR)]
    struct Unity();

    let val = Unity();

    CXS::check_codec(&val, b"");
    TXS::check_codec(&val, b"");
}

#[test]
fn struct_bare() {
    #[derive(Debug, PartialEq, Eq, WriteXDR, ReadXDR)]
    struct Bare;

    let val = Bare;

    CXS::check_codec(&val, b"");
    TXS::check_codec(&val, b"");
}

#[test]
fn newtype() {
    #[derive(Debug, PartialEq, Eq, WriteXDR, ReadXDR)]
    struct New<'a>(&'a [u8; 6]);

    let bytes = [0, 1, 2, 3, 4, 5];
    let val = New(&bytes);

    let expected = &hex!("000102030405");
    CXS::check_codec(&val, expected);

    let expected = &hex!("0001020304050000");
    TXS::check_codec(&val, expected);
}

#[test]
fn field_as_type() {
    #[derive(Debug, PartialEq, Eq)]
    struct Blob(u32);

    impl core::ops::Deref for Blob {
        type Target = u32;

        fn deref(&self) -> &u32 {
            &self.0
        }
    }

    impl From<u32> for Blob {
        fn from(n: u32) -> Self {
            Self(n)
        }
    }

    #[derive(Debug, PartialEq, Eq, WriteXDR, ReadXDR)]
    struct New(#[xdrs(as_type = "u32")] Blob);

    let n: u32 = 7;
    let val = New(Blob::from(n));

    let expected = &hex!("07");
    CXS::check_codec(&val, expected);

    let expected = &hex!("00000007");
    TXS::check_codec(&val, expected);
}

#[test]
fn struct_as_type() {
    #[derive(ReadXDR, WriteXDR)]
    struct Thing(u64);

    // Wrap a u32 and serialize it like a Thing
    #[derive(ReadXDR, WriteXDR, Debug, PartialEq, Eq, Clone)]
    #[xdrs(as_type = "Thing")]
    struct U32(u32);

    impl From<Thing> for U32 {
        fn from(t: Thing) -> Self {
            U32(t.0 as u32)
        }
    }

    impl From<U32> for Thing {
        fn from(u: U32) -> Self {
            Thing(u.0 as u64)
        }
    }

    let val = U32(7);

    let expected = &hex!("07");
    CXS::check_codec(&val, expected);

    let expected = &hex!("0000000000000007");
    TXS::check_codec(&val, expected);
}

#[test]
fn ref_newtype() {
    #[derive(Debug, PartialEq, Eq, WriteXDR)]
    #[repr(transparent)]
    struct Buf([u8; 3]);

    impl Deref for Buf {
        type Target = [u8; 3];

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl<'a> From<&'a [u8; 3]> for &'a Buf {
        fn from(arr: &[u8; 3]) -> Self {
            // SAFETY: #[repr(transparent)] ensures these have the same representation
            let ptr = arr as *const [u8; 3] as *const Buf;
            unsafe { &*ptr }
        }
    }

    impl<'r> ReadXDR<'r> for &'r Buf {
        fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
            let arr: &[u8; 3] = read_xdr(r)?;
            let val = Self::from(arr);
            Ok(val)
        }
    }

    let arr = [0, 1, 2];
    let buf = Buf(arr);

    CXS::check_codec(&&buf, &hex!("000102"));
    TXS::check_codec(&&buf, &hex!("00010200"));

    // make sure we aren't being foxed out by auto(de)ref magic
    let expected = hex!("000102");
    let buf2: &Buf = xdrs::cxs::decode(&expected).unwrap();
    assert_eq!(&buf2.0, &arr);
}

#[test]
fn phantom_data() {
    #[derive(Debug, PartialEq, Eq)]
    struct Erik;

    #[derive(Debug, PartialEq, Eq, ReadXDR, WriteXDR)]
    struct Opera<T> {
        phantom: core::marker::PhantomData<T>,
        christine: u32,
    }

    let opera: Opera<Erik> = Opera {
        phantom: core::marker::PhantomData,
        christine: 5,
    };

    CXS::check_codec(&opera, &hex!("05"));
    TXS::check_codec(&opera, &hex!("00000005"));
}

#[test]
fn struct_with_tuple_field() {
    // Test struct with a simple tuple field
    #[derive(Debug, PartialEq, Eq, WriteXDR, ReadXDR)]
    struct Foo {
        x: (u32, u64),
    }

    let val = Foo {
        x: (42, 1000),
    };

    // u32: 42 (0x0000002A) and u64: 1000 (0x00000000000003E8)
    // CXS: compact format - no padding
    let expected = &hex!("2a03e8");
    CXS::check_codec(&val, expected);

    // TXS: traditional format - padded to 4-byte boundaries
    let expected = &hex!("0000002a00000000000003e8");
    TXS::check_codec(&val, expected);
}

#[test]
fn struct_with_mixed_fields_and_tuple() {
    // Test struct with multiple fields including a tuple
    #[derive(Debug, PartialEq, Eq, WriteXDR, ReadXDR)]
    struct Bar {
        id: u32,
        coords: (i32, i32),
        active: bool,
    }

    let val = Bar {
        id: 100,
        coords: (-10, 20),
        active: true,
    };

    // u32: 100 (0x64), i32: -10 (0xFFFFFFF6), i32: 20 (0x14), bool: true (0x01)
    // CXS: compact format
    let expected = &hex!("643af61401");
    CXS::check_codec(&val, expected);

    // TXS: traditional format
    let expected = &hex!("00000064fffffff60000001400000001");
    TXS::check_codec(&val, expected);
}

#[test]
fn struct_with_nested_tuple() {
    // Test struct with a nested tuple
    #[derive(Debug, PartialEq, Eq, WriteXDR, ReadXDR)]
    struct Nested {
        data: (u32, (i32, bool)),
    }

    let val = Nested {
        data: (5, (-3, false)),
    };

    // u32: 5 (0x05), i32: -3 (0xFFFFFFFD), bool: false (0x00)
    // CXS: compact format
    let expected = &hex!("053d00");
    CXS::check_codec(&val, expected);

    // TXS: traditional format
    let expected = &hex!("00000005fffffffd00000000");
    TXS::check_codec(&val, expected);
}

#[test]
fn struct_with_different_tuple_types() {
    // Test struct with tuples of different primitive types
    #[derive(Debug, PartialEq, Eq, WriteXDR, ReadXDR)]
    struct Multi {
        pair1: (u32, u32),
        pair2: (i64, bool),
    }

    let val = Multi {
        pair1: (0xAABBCCDD, 0x11223344),
        pair2: (9223372036854775807, true), // i64::MAX
    };

    // u32: 0xAABBCCDD, u32: 0x11223344, i64: 0x7FFFFFFFFFFFFFFF, bool: 0x01
    // CXS: compact format
    let expected = &hex!("bfddccbbaabf443322117f7f7f7f7f7f7f7f01");
    CXS::check_codec(&val, expected);

    // TXS: traditional format
    let expected = &hex!("aabbccdd112233447fffffffffffffff00000001");
    TXS::check_codec(&val, expected);
}
