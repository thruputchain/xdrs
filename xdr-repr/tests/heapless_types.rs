#![cfg(feature = "heapless")]

use xdr_repr::*;

fn only_field_type_json<T: HasTypeSpec>() -> String {
    let TypeSpec::Struct(body) = T::type_spec() else {
        panic!("expected embedded struct type spec");
    };
    assert_eq!(body.fields().len(), 1);
    body.fields()[0].get_type_json()
}

#[test]
fn heapless_vec_u8_declares_bounded_opaque() {
    let decl = <heapless::Vec<u8, 8> as Declare>::declare(ident("bytes"));
    assert_eq!(decl.to_string(), "opaque bytes<8>");
    assert_eq!(
        decl.get_type_json(),
        r#"{ "type": "variable opaque", "max": 8 }"#,
    );
    assert_eq!(
        only_field_type_json::<heapless::Vec<u8, 8>>(),
        r#"{ "type": "variable opaque", "max": 8 }"#,
    );
}

#[test]
fn heapless_string_declares_bounded_string() {
    let decl = <heapless::String<16> as Declare>::declare(ident("text"));
    assert_eq!(decl.to_string(), "string text<16>");
    assert_eq!(decl.get_type_json(), r#"{ "type": "string", "max": 16 }"#,);
    assert_eq!(
        only_field_type_json::<heapless::String<16>>(),
        r#"{ "type": "string", "max": 16 }"#,
    );
}

#[test]
fn heapless_vec_generic_declares_bounded_array() {
    let decl = <heapless::Vec<u32, 4> as Declare>::declare(ident("nums"));
    assert_eq!(decl.to_string(), "unsigned int nums<4>");
    assert_eq!(
        decl.get_type_json(),
        r#"{ "type": "variable array", "element_type": { "type": "unsigned" }, "max": 4 }"#,
    );
    assert_eq!(
        only_field_type_json::<heapless::Vec<u32, 4>>(),
        r#"{ "type": "variable array", "element_type": { "type": "unsigned" }, "max": 4 }"#,
    );
}

#[test]
fn borrowed_heapless_declare_forwards_to_owned() {
    assert_eq!(
        <&heapless::Vec<u8, 8> as Declare>::declare(ident("bytes")).to_string(),
        "opaque bytes<8>",
    );
    assert_eq!(
        <&mut heapless::Vec<u8, 8> as Declare>::declare(ident("bytes")).to_string(),
        "opaque bytes<8>",
    );
    assert_eq!(
        <&heapless::String<16> as Declare>::declare(ident("text")).to_string(),
        "string text<16>",
    );
    assert_eq!(
        <&mut heapless::String<16> as Declare>::declare(ident("text")).to_string(),
        "string text<16>",
    );
    assert_eq!(
        <&heapless::Vec<u32, 4> as Declare>::declare(ident("nums")).to_string(),
        "unsigned int nums<4>",
    );
    assert_eq!(
        <&mut heapless::Vec<u32, 4> as Declare>::declare(ident("nums")).to_string(),
        "unsigned int nums<4>",
    );
}
