use xdr_repr::*;

#[test]
fn option_vec_u8_emits_inline_struct_pointer() {
    let decl = <Option<Vec<u8>>>::declare(ident("payload"));
    let formatted = decl.to_string();
    assert_eq!(
        formatted,
        format!("struct {{\n\topaque unnamed_0<>;\n}} *payload"),
        "Option<Vec<u8>> should emit inline struct in pointer position (valid RFC 4506)"
    );
}

#[test]
fn option_tuple_emits_inline_struct_pointer() {
    let decl = <Option<(u32, i32)>>::declare(ident("pair"));
    let formatted = decl.to_string();
    assert_eq!(
        formatted,
        "struct {\n\tunsigned int unnamed_0;\n\tint unnamed_1;\n} *pair",
        "Option<(u32, i32)> should emit inline struct in pointer position (valid RFC 4506)"
    );
}
