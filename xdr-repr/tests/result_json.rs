use xdr_repr::*;

#[test]
fn result_json_is_valid() {
    let json = <Result<bool, u32>>::type_spec().get_type_json();
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("JSON output should be valid for Result");

    assert_eq!(parsed["type"], "union");
    assert_eq!(parsed["discriminant"]["name"], "which");
    assert_eq!(parsed["discriminant"]["element_type"]["type"], "int");

    let cases = parsed["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 2);
    assert_eq!(cases[0]["values"][0], 0);
    assert_eq!(cases[0]["element_type"]["type"], "bool");
    assert_eq!(cases[1]["values"][0], 1);
    assert_eq!(cases[1]["element_type"]["type"], "unsigned");
}

#[test]
fn nested_struct_with_result_json_is_valid() {
    let body = StructBody::new(vec![Declaration::Named {
        name: ident("choice"),
        ty: <Result<bool, u32>>::type_spec(),
    }]);

    let json = TypeSpec::Struct(body).get_type_json();
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("JSON output should be valid for nested Result");

    assert_eq!(parsed["type"], "struct");
    let fields = parsed["fields"].as_array().unwrap();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0]["name"], "choice");
    assert_eq!(fields[0]["element_type"]["type"], "union");
    assert_eq!(
        fields[0]["element_type"]["cases"][0]["element_type"]["type"],
        "bool"
    );
    assert_eq!(
        fields[0]["element_type"]["cases"][1]["element_type"]["type"],
        "unsigned"
    );
}
