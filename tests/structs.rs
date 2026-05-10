mod common;
use common::*;
use core::ops::Deref;
use xdrs::{ReadResult, ReadXDR, Reader, WriteXDR, read_xdr};

#[cfg(feature = "repr")]
mod xdr_field_rename {
    use xdrs::xdr_repr::*;

    #[test]
    fn struct_field_xdr_name_honored() {
        #[allow(dead_code)]
        #[derive(xdrs::XDR)]
        struct Packet {
            #[xdr(name = "wire_count")]
            rust_count: u32,
        }

        let type_def = <Packet as HasTypeDef>::type_def();
        let formatted = Definition::Type(type_def.clone()).to_string();

        let TypeDef::Struct(_, body) = &type_def else {
            panic!("derive should emit a struct type definition");
        };
        let fields = body.fields();

        assert_eq!(fields.len(), 1);
        let Declaration::Named { name, .. } = &fields[0] else {
            panic!("expected a named declaration");
        };

        assert_eq!(name.0, "wire_count");
        assert!(formatted.contains("unsigned int wire_count;"));
        assert!(!formatted.contains("rust_count"));
    }

    #[test]
    fn struct_field_without_rename_uses_rust_name() {
        #[allow(dead_code)]
        #[derive(xdrs::XDR)]
        struct Packet {
            count: u32,
        }

        let type_def = <Packet as HasTypeDef>::type_def();
        let formatted = Definition::Type(type_def).to_string();
        assert!(formatted.contains("unsigned int count;"));
    }
}

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
fn struct_into_from_type() {
    // Serialize a u8 as a u32

    #[derive(Debug, Clone, PartialEq, Eq, ReadXDR, WriteXDR)]
    #[xdrs(into_type = "u32", from_type = "u32")]
    struct U32(u8);

    impl Into<u32> for U32 {
        fn into(self) -> u32 {
            self.0.into()
        }
    }

    impl From<u32> for U32 {
        fn from(n: u32) -> Self {
            Self(n.try_into().unwrap())
        }
    }

    let val = U32(7);

    let expected = &hex!("07");
    CXS::check_codec(&val, expected);

    let expected = &hex!("00000007");
    TXS::check_codec(&val, expected);
}

#[test]
fn struct_via_type() {
    // Serialize a u8 as a u32

    #[derive(Debug, Clone, PartialEq, Eq, ReadXDR, WriteXDR)]
    #[xdrs(via = "u32")]
    struct U32(u8);

    impl Into<u32> for U32 {
        fn into(self) -> u32 {
            self.0.into()
        }
    }

    impl From<u32> for U32 {
        fn from(n: u32) -> Self {
            Self(n.try_into().unwrap())
        }
    }

    let val = U32(7);

    let expected = &hex!("07");
    CXS::check_codec(&val, expected);

    let expected = &hex!("00000007");
    TXS::check_codec(&val, expected);
}

#[test]
fn field_into_from_type() {
    // Something that doesn't impl *XDR, perhaps a foreign type
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct U32(u32);

    impl From<u32> for U32 {
        fn from(n: u32) -> Self {
            Self(n)
        }
    }

    impl Into<u32> for U32 {
        fn into(self) -> u32 {
            self.0
        }
    }

    #[derive(Debug, PartialEq, Eq, WriteXDR, ReadXDR)]
    struct Container {
        #[xdrs(into_type = "u32", from_type = "u32")]
        item: U32,
    }

    let val = Container { item: U32(7) };

    let expected = &hex!("07");
    CXS::check_codec(&val, expected);

    let expected = &hex!("00000007");
    TXS::check_codec(&val, expected);
}

#[test]
fn field_via_type() {
    // Something that doesn't impl *XDR, perhaps a foreign type
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct U32(u32);

    impl From<u32> for U32 {
        fn from(n: u32) -> Self {
            Self(n)
        }
    }

    impl Into<u32> for U32 {
        fn into(self) -> u32 {
            self.0
        }
    }

    #[derive(Debug, PartialEq, Eq, WriteXDR, ReadXDR)]
    struct Container {
        #[xdrs(via = "u32")]
        item: U32,
    }

    let val = Container { item: U32(7) };

    let expected = &hex!("07");
    CXS::check_codec(&val, expected);

    let expected = &hex!("00000007");
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
fn phantom_lifetime_does_not_borrow_from_reader() {
    #[derive(Debug, PartialEq, Eq, ReadXDR, WriteXDR)]
    struct Marker<'a> {
        phantom: core::marker::PhantomData<&'a ()>,
        value: u32,
    }

    fn decode_marker(bytes: &[u8]) -> Marker<'static> {
        xdrs::txs::decode(bytes).unwrap()
    }

    let marker = Marker {
        phantom: core::marker::PhantomData,
        value: 5,
    };

    assert_eq!(decode_marker(&hex!("00000005")), marker);
}

#[test]
fn struct_with_tuple_field() {
    // Test struct with a simple tuple field
    #[derive(Debug, PartialEq, Eq, WriteXDR, ReadXDR)]
    struct Foo {
        x: (u32, u64),
    }

    let val = Foo { x: (42, 1000) };

    // u32: 42 (≤63, no prefix) and u64: 1000 (0x03E8, needs 0x7F+2-byte)
    // CXS: 42="2a", 1000="7fe803" (little-endian)
    let expected = &hex!("2a7fe803");
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

    // u32: 100 (0x64, range 32-255 needs 0x3F prefix), i32: -10 (compact: 63-10=53=0x35),
    // i32: 20 (0x14, single byte), bool: true (0x01)
    // CXS: 100="3f64", -10="35", 20="14", true="01"
    let expected = &hex!("3f64351401");
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

    // u32: 5 (0x05), i32: -3 (compact: 63-3=60=0x3C), bool: false (0x00)
    // CXS: 5="05", -3="3c", false="00"
    let expected = &hex!("053c00");
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

    // u32: 0xAABBCCDD (4-byte: 0xBF+little-endian), u32: 0x11223344 (4-byte),
    // i64: 0x7FFFFFFFFFFFFFFF (8-byte: 0xFF+little-endian), bool: 0x01
    // CXS: 0xBF+DDCCBBAA, 0xBF+44332211, 0xFF+FFFFFFFFFFFFFF7F, 01
    let expected = &hex!("bfddccbbaabf44332211ffffffffffffffff7f01");
    CXS::check_codec(&val, expected);

    // TXS: traditional format
    let expected = &hex!("aabbccdd112233447fffffffffffffff00000001");
    TXS::check_codec(&val, expected);
}
