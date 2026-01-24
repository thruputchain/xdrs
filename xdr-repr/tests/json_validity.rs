use xdr_repr::*;
use xdrs::*;

// Tests to ensure JSON output is valid and parseable
// These tests validate the JSON structure using serde_json parsing

#[test]
fn void_json_is_valid() {
    let decl = Declaration::Void;
    let json = decl.get_type_json();

    // Parse the JSON to ensure it's valid
    let parsed: serde_json::Value = serde_json::from_str(&json)
        .expect("JSON output should be valid");

    assert_eq!(parsed["type"], "void");
}

#[test]
fn primitive_types_json_is_valid() {
    let test_cases = vec![
        (TypeSpec::Int, "int"),
        (TypeSpec::Unsigned, "unsigned"),
        (TypeSpec::Hyper, "hyper"),
        (TypeSpec::UnsignedHyper, "unsigned hyper"),
        (TypeSpec::Float, "float"),
        (TypeSpec::Double, "double"),
        (TypeSpec::Quadruple, "quadruple"),
        (TypeSpec::Bool, "bool"),
    ];

    for (type_spec, expected_name) in test_cases {
        let decl = Declaration::Named {
            name: ident("test"),
            ty: type_spec,
        };
        let json = decl.get_type_json();

        let parsed: serde_json::Value = serde_json::from_str(&json)
            .expect(&format!("JSON output should be valid for {}", expected_name));

        assert_eq!(parsed["type"], expected_name);
    }
}

#[test]
fn fixed_opaque_json_is_valid() {
    let decl = declare_fixed_opaque("data", 10);
    let json = decl.get_type_json();

    let parsed: serde_json::Value = serde_json::from_str(&json)
        .expect("JSON output should be valid");

    assert_eq!(parsed["type"], "fixed opaque");
    assert_eq!(parsed["len"], 10);
}

#[test]
fn variable_opaque_json_is_valid() {
    let decl = Declaration::VariableOpaque {
        name: ident("data"),
        max: unsigned(100),
    };
    let json = decl.get_type_json();

    let parsed: serde_json::Value = serde_json::from_str(&json)
        .expect("JSON output should be valid");

    assert_eq!(parsed["type"], "variable opaque");
    assert_eq!(parsed["max"], 100);
}

#[test]
fn string_json_is_valid() {
    let decl = Declaration::String {
        name: ident("text"),
        max: unsigned(255),
    };
    let json = decl.get_type_json();

    let parsed: serde_json::Value = serde_json::from_str(&json)
        .expect("JSON output should be valid");

    assert_eq!(parsed["type"], "string");
    assert_eq!(parsed["max"], 255);
}

#[test]
fn fixed_array_json_is_valid() {
    let decl = Declaration::FixedArray {
        name: ident("items"),
        elt_ty: TypeSpec::Unsigned,
        len: unsigned(5),
    };
    let json = decl.get_type_json();

    let parsed: serde_json::Value = serde_json::from_str(&json)
        .expect("JSON output should be valid");

    assert_eq!(parsed["type"], "fixed array");
    assert_eq!(parsed["elt_ty"]["type"], "unsigned");
    assert_eq!(parsed["len"], 5);
}

#[test]
fn variable_array_json_is_valid() {
    let decl = Declaration::VariableArray {
        name: ident("items"),
        elt_ty: TypeSpec::Int,
        max: unsigned(10),
    };
    let json = decl.get_type_json();

    let parsed: serde_json::Value = serde_json::from_str(&json)
        .expect("JSON output should be valid");

    assert_eq!(parsed["type"], "variable array");
    assert_eq!(parsed["elt_ty"]["type"], "int");
    assert_eq!(parsed["max"], 10);
}

#[test]
fn optional_json_is_valid() {
    let decl = Declaration::Optional {
        name: ident("value"),
        opt_ty: TypeSpec::Bool,
    };
    let json = decl.get_type_json();

    let parsed: serde_json::Value = serde_json::from_str(&json)
        .expect("JSON output should be valid");

    assert_eq!(parsed["type"], "optional");
    assert_eq!(parsed["opt_ty"]["type"], "bool");
}

#[test]
fn struct_json_is_valid() {
    #[derive(XDR)]
    struct TestStruct {
        _id: u32,
        _data: [u8; 16],
        _count: i32,
    }

    let type_def = TestStruct::type_def();
    if let TypeDef::Struct(_, body) = type_def {
        let json = TypeSpec::Struct(body).get_type_json();

        let parsed: serde_json::Value = serde_json::from_str(&json)
            .expect("JSON output should be valid");

        assert_eq!(parsed["type"], "struct");
        assert!(parsed["fields"].is_array());
        let fields = parsed["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 3);

        // Validate first field
        assert_eq!(fields[0]["name"], "_id");
        assert_eq!(fields[0]["field_type"]["type"], "unsigned");

        // Validate second field
        assert_eq!(fields[1]["name"], "_data");
        assert_eq!(fields[1]["field_type"]["type"], "fixed opaque");
        assert_eq!(fields[1]["field_type"]["len"], 16);

        // Validate third field
        assert_eq!(fields[2]["name"], "_count");
        assert_eq!(fields[2]["field_type"]["type"], "int");
    } else {
        panic!("Expected TypeDef::Struct");
    }
}

#[test]
fn nested_struct_json_is_valid() {
    #[derive(XDR)]
    struct Inner {
        _value: u32,
    }

    #[derive(XDR)]
    struct Outer {
        _inner: Inner,
        _flag: bool,
    }

    let type_def = Outer::type_def();
    if let TypeDef::Struct(_, body) = type_def {
        let json = TypeSpec::Struct(body).get_type_json();

        let parsed: serde_json::Value = serde_json::from_str(&json)
            .expect("JSON output should be valid for nested struct");

        assert_eq!(parsed["type"], "struct");
        let fields = parsed["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 2);

        // Validate nested struct field - when using a custom type, it's referenced by name
        assert_eq!(fields[0]["name"], "_inner");
        assert_eq!(fields[0]["field_type"]["type"], "Inner");

        // Validate boolean field
        assert_eq!(fields[1]["name"], "_flag");
        assert_eq!(fields[1]["field_type"]["type"], "bool");
    } else {
        panic!("Expected TypeDef::Struct");
    }
}

#[test]
fn custom_type_name_json_is_valid() {
    let decl = Declaration::Named {
        name: ident("value"),
        ty: TypeSpec::Ident(ident("CustomType")),
    };
    let json = decl.get_type_json();

    let parsed: serde_json::Value = serde_json::from_str(&json)
        .expect("JSON output should be valid");

    assert_eq!(parsed["type"], "CustomType");
}

#[test]
fn escaped_identifier_json_is_valid() {
    // Test that identifiers with special characters are properly escaped
    let special_names = vec![
        "Type\"WithQuotes",
        "Type\\WithBackslash",
        "Type\nWithNewline",
        "Type\tWithTab",
        "Type\rWithCarriageReturn",
    ];

    for name in special_names {
        let decl = Declaration::Named {
            name: ident(name),
            ty: TypeSpec::Ident(ident(name)),
        };
        let json = decl.get_type_json();

        // The key test: the JSON must be parseable despite special characters
        let parsed: serde_json::Value = serde_json::from_str(&json)
            .expect(&format!("JSON should be valid even with special chars in identifier: {:?}", name));

        // The parsed value should match the original (serde_json handles unescaping)
        assert_eq!(parsed["type"], name);
    }
}

#[test]
fn unicode_identifier_json_is_valid() {
    // Test Unicode identifiers
    let unicode_names = vec![
        "Type_中文",
        "Type_Русский",
        "Type_🎉",
        "Type_العربية",
    ];

    for name in unicode_names {
        let decl = Declaration::Named {
            name: ident(name),
            ty: TypeSpec::Ident(ident(name)),
        };
        let json = decl.get_type_json();

        let parsed: serde_json::Value = serde_json::from_str(&json)
            .expect(&format!("JSON should be valid with Unicode identifier: {:?}", name));

        assert_eq!(parsed["type"], name);
    }
}

#[test]
fn empty_struct_json_is_valid() {
    let body = StructBody(vec![]);
    let json = TypeSpec::Struct(body).get_type_json();

    let parsed: serde_json::Value = serde_json::from_str(&json)
        .expect("JSON output should be valid for empty struct");

    assert_eq!(parsed["type"], "struct");
    assert_eq!(parsed["fields"].as_array().unwrap().len(), 0);
}

#[test]
fn deeply_nested_arrays_json_is_valid() {
    #[derive(XDR)]
    struct Tensor {
        _data: [[[u32; 2]; 3]; 4],
    }

    let type_def = Tensor::type_def();
    if let TypeDef::Struct(_, body) = type_def {
        let json = body.0[0].get_type_json();

        // This is a complex nested structure - ensure it parses
        let parsed: serde_json::Value = serde_json::from_str(&json)
            .expect("JSON output should be valid for deeply nested arrays");

        // Validate the outermost array
        assert_eq!(parsed["type"], "fixed array");
        assert_eq!(parsed["len"], 4);

        // Validate the middle level (embedded struct with array)
        assert_eq!(parsed["elt_ty"]["type"], "struct");
        let middle_fields = parsed["elt_ty"]["fields"].as_array().unwrap();
        assert_eq!(middle_fields[0]["field_type"]["type"], "fixed array");
        assert_eq!(middle_fields[0]["field_type"]["len"], 3);

        // Validate the innermost level
        let inner_elt_ty = &middle_fields[0]["field_type"]["elt_ty"];
        assert_eq!(inner_elt_ty["type"], "struct");
    } else {
        panic!("Expected TypeDef::Struct");
    }
}

#[test]
fn tuple_json_is_valid() {
    let decl = <(i32, u32, bool)>::declare(ident("triple"));
    let json = decl.get_type_json();

    let parsed: serde_json::Value = serde_json::from_str(&json)
        .expect("JSON output should be valid for tuple");

    assert_eq!(parsed["type"], "struct");
    let fields = parsed["fields"].as_array().unwrap();
    assert_eq!(fields.len(), 3);

    assert_eq!(fields[0]["name"], "unnamed_0");
    assert_eq!(fields[0]["field_type"]["type"], "int");

    assert_eq!(fields[1]["name"], "unnamed_1");
    assert_eq!(fields[1]["field_type"]["type"], "unsigned");

    assert_eq!(fields[2]["name"], "unnamed_2");
    assert_eq!(fields[2]["field_type"]["type"], "bool");
}

#[test]
fn byte_array_json_is_valid() {
    let decl = <[u8; 32]>::declare(ident("hash"));
    let json = decl.get_type_json();

    let parsed: serde_json::Value = serde_json::from_str(&json)
        .expect("JSON output should be valid for byte array");

    assert_eq!(parsed["type"], "fixed opaque");
    assert_eq!(parsed["len"], 32);
}

#[test]
fn typed_array_json_is_valid() {
    let decl = <[u32; 10]>::declare(ident("numbers"));
    let json = decl.get_type_json();

    let parsed: serde_json::Value = serde_json::from_str(&json)
        .expect("JSON output should be valid for typed array");

    assert_eq!(parsed["type"], "fixed array");
    assert_eq!(parsed["elt_ty"]["type"], "unsigned");
    assert_eq!(parsed["len"], 10);
}

#[test]
fn complex_field_names_json_is_valid() {
    // Test that field names with special characters are properly escaped
    // Build a struct manually with special character field names
    let body = StructBody(vec![
        Declaration::Named {
            name: ident("field\"with\"quotes"),
            ty: TypeSpec::Unsigned,
        },
        Declaration::Named {
            name: ident("field\\with\\backslashes"),
            ty: TypeSpec::Unsigned,
        },
        Declaration::Named {
            name: ident("field\nwith\nnewlines"),
            ty: TypeSpec::Unsigned,
        },
    ]);

    let json = TypeSpec::Struct(body).get_type_json();

    // The critical test: JSON must be parseable despite special chars in field names
    let parsed: serde_json::Value = serde_json::from_str(&json)
        .expect("JSON should be valid even with special chars in field names");

    assert_eq!(parsed["type"], "struct");
    let fields = parsed["fields"].as_array().unwrap();
    assert_eq!(fields.len(), 3);

    // Verify the field names were properly escaped and unescaped
    assert_eq!(fields[0]["name"], "field\"with\"quotes");
    assert_eq!(fields[1]["name"], "field\\with\\backslashes");
    assert_eq!(fields[2]["name"], "field\nwith\nnewlines");
}
