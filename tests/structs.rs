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
fn as_type() {
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
    let buf2: &Buf = xdrs::serialize_cxs::decode(&expected).unwrap();
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
