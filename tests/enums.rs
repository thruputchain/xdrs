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

    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
    #[repr(i32)]
    enum Foo<'a> {
        A(&'a [u8]) = FOO_A,
        B { x: i32, y: u32 } = 7,
        C = 2,
        Default(i32), // sole implicit arm with one i32 field is a default
    }

    let bytes = b"\r\n";
    let val = Foo::A(bytes);
    CXS::check_codec(&val, &hex!("3e020d0a"));

    let val = Foo::B { x: 0, y: 1 };
    CXS::check_codec(&val, &hex!("070001"));

    let val = Foo::C;
    CXS::check_codec(&val, &hex!("02"));
    TXS::check_codec(&val, &hex!("00000002"));

    let val = Foo::Default(3);
    CXS::check_codec(&val, &hex!("03"));
    TXS::check_codec(&val, &hex!("00000003"));

    let val = Foo::Default(10);
    CXS::check_decode(&val, &hex!("0a"));
    TXS::check_decode(&val, &hex!("0000000a"));
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

#[test]
fn enum_default_variant_encode() {
    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
    #[repr(i32)]
    enum FooDefault {
        A { a: u32 } = 1,
        B { b: i32 } = 2,
        C = 3,
        Default { d: i32 },
    }

    let discriminant: i32 = 0x0ABBCCDD;
    CXS::check_encode(&discriminant, &hex!("bfddccbb0a"));
    TXS::check_encode(&discriminant, &hex!("0ABBCCDD"));

    let val = FooDefault::Default { d: discriminant };
    CXS::check_codec(&val, &hex!("bfddccbb0a"));
    TXS::check_codec(&val, &hex!("0ABBCCDD"));
}

#[test]
fn enum_default_variant_with_payload_roundtrip() {
    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
    #[repr(i32)]
    enum UnnamedPayload {
        Known = 0,
        Default(i32, u32),
    }

    let original = UnnamedPayload::Default(7, 9);
    TXS::check_codec(&original, &hex!("0000000700000009"));

    let cxs_encoded = xdrs::cxs::encode(&original).unwrap();
    let cxs_decoded: UnnamedPayload = xdrs::cxs::decode(&cxs_encoded).unwrap();
    assert_eq!(cxs_decoded, original);

    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
    #[repr(i32)]
    enum NamedPayload {
        Known { val: u32 } = 0,
        Default { code: i32, data: u32 },
    }

    let original = NamedPayload::Default { code: 7, data: 9 };
    TXS::check_codec(&original, &hex!("0000000700000009"));

    let cxs_encoded = xdrs::cxs::encode(&original).unwrap();
    let cxs_decoded: NamedPayload = xdrs::cxs::decode(&cxs_encoded).unwrap();
    assert_eq!(cxs_decoded, original);
}

// We don't support enums with mixed implicit/explicit variants
// (except that a default arm is allowed).
//
// This test should produce a compile error.
#[cfg(any())]
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

#[cfg(feature = "repr")]
mod external_discrim {
    use super::*;
    use xdrs::xdr_repr::*;

    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR, xdrs::XDR)]
    #[repr(i32)]
    enum Status {
        Ready = 10,
        Failed = 11,
    }

    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR, xdrs::XDR)]
    #[xdr(discrim_type = "Status")]
    #[repr(i32)]
    enum Message {
        Ready { value: u32 } = Status::Ready as i32,
        Failed { code: u32 } = Status::Failed as i32,
    }

    #[test]
    fn enum_external_discrim_explicit() {
        // Status::Ready = 10 = 0x0a, value = 7
        let ready = Message::Ready { value: 7 };
        TXS::check_codec(&ready, &hex!("0000000a00000007"));
        CXS::check_codec(&ready, &hex!("0a07"));

        // Status::Failed = 11 = 0x0b, code = 3
        let failed = Message::Failed { code: 3 };
        TXS::check_codec(&failed, &hex!("0000000b00000003"));
        CXS::check_codec(&failed, &hex!("0b03"));
    }
}
