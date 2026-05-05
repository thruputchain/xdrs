use xdr_repr::*;
use xdrs::*;

#[test]
fn slice_u8_has_type_spec() {
    let ts = <&[u8]>::type_spec();
    let TypeSpec::Struct(body) = ts else {
        panic!("expected TypeSpec::Struct, got {ts:?}");
    };
    assert_eq!(body.0.len(), 1);
    let decl = &body.0[0];
    assert_eq!(
        decl.get_type_json(),
        format!(r#"{{ "type": "variable opaque", "max": {} }}"#, u32::MAX),
    );
}

#[test]
fn vec_u8_has_type_spec() {
    let ts = <Vec<u8>>::type_spec();
    let TypeSpec::Struct(body) = ts else {
        panic!("expected TypeSpec::Struct, got {ts:?}");
    };
    assert_eq!(body.0.len(), 1);
    let decl = &body.0[0];
    assert_eq!(
        decl.get_type_json(),
        format!(r#"{{ "type": "variable opaque", "max": {} }}"#, u32::MAX),
    );
}

#[test]
fn bytes_m_has_type_spec() {
    let ts = <BytesM<256>>::type_spec();
    let TypeSpec::Struct(body) = ts else {
        panic!("expected TypeSpec::Struct, got {ts:?}");
    };
    assert_eq!(body.0.len(), 1);
    let decl = &body.0[0];
    assert_eq!(
        decl.get_type_json(),
        r#"{ "type": "variable opaque", "max": 256 }"#,
    );
}

#[test]
fn vec_m_u8_has_type_spec() {
    let ts = <VecM<u8, 1024>>::type_spec();
    let TypeSpec::Struct(body) = ts else {
        panic!("expected TypeSpec::Struct, got {ts:?}");
    };
    assert_eq!(body.0.len(), 1);
    let decl = &body.0[0];
    assert_eq!(
        decl.get_type_json(),
        r#"{ "type": "variable opaque", "max": 1024 }"#,
    );
}

#[test]
fn option_vec_u8_has_type_spec() {
    let ts = <Option<Vec<u8>>>::type_spec();
    let TypeSpec::Struct(body) = ts else {
        panic!("expected TypeSpec::Struct, got {ts:?}");
    };
    assert_eq!(body.0.len(), 1);
    let json = body.0[0].get_type_json();
    assert!(json.contains("optional"), "expected optional in: {json}");
    assert!(
        json.contains("variable opaque"),
        "expected variable opaque in: {json}"
    );
}

#[test]
fn option_slice_u8_has_type_spec() {
    let ts = <Option<&[u8]>>::type_spec();
    let TypeSpec::Struct(body) = ts else {
        panic!("expected TypeSpec::Struct, got {ts:?}");
    };
    assert_eq!(body.0.len(), 1);
    let json = body.0[0].get_type_json();
    assert!(json.contains("optional"), "expected optional in: {json}");
    assert!(
        json.contains("variable opaque"),
        "expected variable opaque in: {json}"
    );
}

#[test]
fn option_bytes_m_has_type_spec() {
    let ts = <Option<BytesM<512>>>::type_spec();
    let TypeSpec::Struct(body) = ts else {
        panic!("expected TypeSpec::Struct, got {ts:?}");
    };
    assert_eq!(body.0.len(), 1);
    let json = body.0[0].get_type_json();
    assert!(json.contains("optional"), "expected optional in: {json}");
    assert!(
        json.contains("variable opaque"),
        "expected variable opaque in: {json}"
    );
    assert!(json.contains("512"), "expected max 512 in: {json}");
}

#[test]
fn option_vec_m_u8_has_type_spec() {
    let ts = <Option<VecM<u8, 100>>>::type_spec();
    let TypeSpec::Struct(body) = ts else {
        panic!("expected TypeSpec::Struct, got {ts:?}");
    };
    assert_eq!(body.0.len(), 1);
    let json = body.0[0].get_type_json();
    assert!(json.contains("optional"), "expected optional in: {json}");
    assert!(
        json.contains("variable opaque"),
        "expected variable opaque in: {json}"
    );
    assert!(json.contains("100"), "expected max 100 in: {json}");
}

#[test]
fn declare_option_vec_u8() {
    let decl = <Option<Vec<u8>>>::declare(ident("payload"));
    let json = decl.get_type_json();
    assert!(json.contains("optional"), "expected optional in: {json}");
    assert!(
        json.contains("variable opaque"),
        "expected variable opaque in: {json}"
    );
}

#[test]
fn declare_option_bytes_m() {
    let decl = <Option<BytesM<64>>>::declare(ident("data"));
    let json = decl.get_type_json();
    assert!(json.contains("optional"), "expected optional in: {json}");
    assert!(
        json.contains("variable opaque"),
        "expected variable opaque in: {json}"
    );
    assert!(json.contains("64"), "expected max 64 in: {json}");
}
