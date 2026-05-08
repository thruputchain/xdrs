use xdr_repr::*;

#[test]
fn borrowed_fixed_byte_array_declare() {
    let decl = <&[u8; 32] as Declare>::declare(ident("hash"));
    assert_eq!(decl.to_string(), "opaque hash[32]");
}

#[test]
fn borrowed_fixed_byte_array_type_spec() {
    let ts = <&[u8; 32]>::type_spec();
    let TypeSpec::Struct(body) = ts else {
        panic!("expected TypeSpec::Struct, got {ts:?}");
    };
    assert_eq!(body.fields().len(), 1);
    assert_eq!(
        body.fields()[0].get_type_json(),
        r#"{ "type": "fixed opaque", "len": 32 }"#,
    );
}

#[test]
fn mut_borrowed_fixed_byte_array_declare() {
    let decl = <&mut [u8; 16] as Declare>::declare(ident("key"));
    assert_eq!(decl.to_string(), "opaque key[16]");
}
