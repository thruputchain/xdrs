use xdr_repr::*;
use xdrs::*;

// Tests to ensure JSON output is valid and parseable
// These tests validate the JSON structure using serde_json parsing

#[test]
fn void_json_is_valid() {
    let decl = Declaration::Void;
    let json = decl.get_type_json();

    // Parse the JSON to ensure it's valid
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("JSON output should be valid");

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

        let parsed: serde_json::Value = serde_json::from_str(&json).expect(&format!(
            "JSON output should be valid for {}",
            expected_name
        ));

        assert_eq!(parsed["type"], expected_name);
    }
}

#[test]
fn fixed_opaque_json_is_valid() {
    let decl = declare_fixed_opaque("data", 10);
    let json = decl.get_type_json();

    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("JSON output should be valid");

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

    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("JSON output should be valid");

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

    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("JSON output should be valid");

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

    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("JSON output should be valid");

    assert_eq!(parsed["type"], "fixed array");
    assert_eq!(parsed["element_type"]["type"], "unsigned");
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

    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("JSON output should be valid");

    assert_eq!(parsed["type"], "variable array");
    assert_eq!(parsed["element_type"]["type"], "int");
    assert_eq!(parsed["max"], 10);
}

#[test]
fn optional_json_is_valid() {
    let decl = Declaration::Optional {
        name: ident("value"),
        opt_ty: TypeSpec::Bool,
    };
    let json = decl.get_type_json();

    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("JSON output should be valid");

    assert_eq!(parsed["type"], "optional");
    assert_eq!(parsed["element_type"]["type"], "bool");
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

        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("JSON output should be valid");

        assert_eq!(parsed["type"], "struct");
        assert!(parsed["fields"].is_array());
        let fields = parsed["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 3);

        // Validate first field
        assert_eq!(fields[0]["name"], "_id");
        assert_eq!(fields[0]["element_type"]["type"], "unsigned");

        // Validate second field
        assert_eq!(fields[1]["name"], "_data");
        assert_eq!(fields[1]["element_type"]["type"], "fixed opaque");
        assert_eq!(fields[1]["element_type"]["len"], 16);

        // Validate third field
        assert_eq!(fields[2]["name"], "_count");
        assert_eq!(fields[2]["element_type"]["type"], "int");
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

        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("JSON output should be valid for nested struct");

        assert_eq!(parsed["type"], "struct");
        let fields = parsed["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 2);

        // Validate nested struct field - when using a custom type, it's referenced by name
        assert_eq!(fields[0]["name"], "_inner");
        assert_eq!(fields[0]["element_type"]["type"], "Inner");

        // Validate boolean field
        assert_eq!(fields[1]["name"], "_flag");
        assert_eq!(fields[1]["element_type"]["type"], "bool");
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

    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("JSON output should be valid");

    assert_eq!(parsed["type"], "CustomType");
}

#[test]
#[should_panic(expected = "Invalid XDR identifier")]
fn invalid_identifier_with_quotes_panics() {
    // XDR identifiers cannot contain quotes
    let decl = Declaration::Named {
        name: ident("valid_name"),
        ty: TypeSpec::Ident(ident("Type\"WithQuotes")),
    };
    let _ = decl.get_type_json(); // Should panic
}

#[test]
#[should_panic(expected = "Invalid XDR identifier")]
fn invalid_identifier_with_backslash_panics() {
    // XDR identifiers cannot contain backslashes
    let decl = Declaration::Named {
        name: ident("valid_name"),
        ty: TypeSpec::Ident(ident("Type\\WithBackslash")),
    };
    let _ = decl.get_type_json(); // Should panic
}

#[test]
#[should_panic(expected = "Invalid XDR identifier")]
fn invalid_identifier_with_newline_panics() {
    // XDR identifiers cannot contain newlines
    let decl = Declaration::Named {
        name: ident("valid_name"),
        ty: TypeSpec::Ident(ident("Type\nWithNewline")),
    };
    let _ = decl.get_type_json(); // Should panic
}

#[test]
#[should_panic(expected = "Invalid XDR identifier")]
fn invalid_identifier_starting_with_digit_panics() {
    // XDR identifiers must start with a letter
    let decl = Declaration::Named {
        name: ident("valid_name"),
        ty: TypeSpec::Ident(ident("9InvalidStart")),
    };
    let _ = decl.get_type_json(); // Should panic
}

#[test]
#[should_panic(expected = "Invalid XDR identifier")]
fn invalid_identifier_with_dash_panics() {
    // XDR identifiers cannot contain dashes
    let decl = Declaration::Named {
        name: ident("valid_name"),
        ty: TypeSpec::Ident(ident("Invalid-Name")),
    };
    let _ = decl.get_type_json(); // Should panic
}

#[test]
#[should_panic(expected = "Invalid XDR identifier")]
fn unicode_identifier_panics() {
    // XDR identifiers must be ASCII only, no Unicode allowed
    let decl = Declaration::Named {
        name: ident("valid_name"),
        ty: TypeSpec::Ident(ident("Type_🎉")),
    };
    let _ = decl.get_type_json(); // Should panic
}

#[test]
fn valid_identifiers_work() {
    // Test that valid XDR identifiers work correctly
    let valid_names = vec![
        "SimpleType",
        "Type123",
        "type_with_underscores",
        "T",
        "a1b2c3",
        "UPPERCASE_TYPE",
    ];

    for name in valid_names {
        let decl = Declaration::Named {
            name: ident("field"),
            ty: TypeSpec::Ident(ident(name)),
        };
        let json = decl.get_type_json();

        let parsed: serde_json::Value = serde_json::from_str(&json).expect(&format!(
            "JSON should be valid for valid identifier: {:?}",
            name
        ));

        assert_eq!(parsed["type"], name);
    }
}

#[test]
fn empty_struct_json_is_valid() {
    let body = StructBody(vec![]);
    let json = TypeSpec::Struct(body).get_type_json();

    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("JSON output should be valid for empty struct");

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
        assert_eq!(parsed["element_type"]["type"], "struct");
        let middle_fields = parsed["element_type"]["fields"].as_array().unwrap();
        assert_eq!(middle_fields[0]["element_type"]["type"], "fixed array");
        assert_eq!(middle_fields[0]["element_type"]["len"], 3);

        // Validate the innermost level
        let inner_elt_ty = &middle_fields[0]["element_type"]["element_type"];
        assert_eq!(inner_elt_ty["type"], "struct");
    } else {
        panic!("Expected TypeDef::Struct");
    }
}

#[test]
fn tuple_json_is_valid() {
    let decl = <(i32, u32, bool)>::declare(ident("triple"));
    let json = decl.get_type_json();

    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("JSON output should be valid for tuple");

    assert_eq!(parsed["type"], "struct");
    let fields = parsed["fields"].as_array().unwrap();
    assert_eq!(fields.len(), 3);

    assert_eq!(fields[0]["name"], "unnamed_0");
    assert_eq!(fields[0]["element_type"]["type"], "int");

    assert_eq!(fields[1]["name"], "unnamed_1");
    assert_eq!(fields[1]["element_type"]["type"], "unsigned");

    assert_eq!(fields[2]["name"], "unnamed_2");
    assert_eq!(fields[2]["element_type"]["type"], "bool");
}

#[test]
fn byte_array_json_is_valid() {
    let decl = <[u8; 32]>::declare(ident("hash"));
    let json = decl.get_type_json();

    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("JSON output should be valid for byte array");

    assert_eq!(parsed["type"], "fixed opaque");
    assert_eq!(parsed["len"], 32);
}

#[test]
fn typed_array_json_is_valid() {
    let decl = <[u32; 10]>::declare(ident("numbers"));
    let json = decl.get_type_json();

    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("JSON output should be valid for typed array");

    assert_eq!(parsed["type"], "fixed array");
    assert_eq!(parsed["element_type"]["type"], "unsigned");
    assert_eq!(parsed["len"], 10);
}

#[test]
#[should_panic(expected = "Invalid XDR identifier")]
fn invalid_field_name_panics() {
    // Field names with special characters should cause a panic
    let body = StructBody(vec![Declaration::Named {
        name: ident("field-with-dashes"), // Invalid: contains dashes
        ty: TypeSpec::Unsigned,
    }]);

    let _ = TypeSpec::Struct(body).get_type_json(); // Should panic
}

#[test]
fn valid_field_names_work() {
    // Test that valid XDR field names work correctly
    let body = StructBody(vec![
        Declaration::Named {
            name: ident("field_one"),
            ty: TypeSpec::Unsigned,
        },
        Declaration::Named {
            name: ident("field2"),
            ty: TypeSpec::Int,
        },
        Declaration::Named {
            name: ident("FIELD_THREE"),
            ty: TypeSpec::Bool,
        },
    ]);

    let json = TypeSpec::Struct(body).get_type_json();

    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("JSON should be valid for valid field names");

    assert_eq!(parsed["type"], "struct");
    let fields = parsed["fields"].as_array().unwrap();
    assert_eq!(fields.len(), 3);

    assert_eq!(fields[0]["name"], "field_one");
    assert_eq!(fields[1]["name"], "field2");
    assert_eq!(fields[2]["name"], "FIELD_THREE");
}

#[test]
fn embedded_union_json_is_valid() {
    let union = TypeSpec::Union(std::boxed::Box::new(UnionBody {
        discriminant_decl: declare_int("which"),
        case_specs: vec![
            CaseSpec {
                cases: vec![signed(0)],
                decl: declare_type("ok", TypeSpec::Bool),
            },
            CaseSpec {
                cases: vec![signed(1)],
                decl: declare_unsigned("err"),
            },
        ],
        default_decl: None,
    }));

    let json = union.get_type_json();
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("JSON output should be valid for embedded union");

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
fn embedded_union_identifier_cases_and_default_json_are_valid() {
    let union = TypeSpec::Union(std::boxed::Box::new(UnionBody {
        discriminant_decl: declare_int("which"),
        case_specs: vec![
            CaseSpec {
                cases: vec![Value::Ident(ident("RECOVERABLE"))],
                decl: declare_type("ok", TypeSpec::Bool),
            },
            CaseSpec {
                cases: vec![Value::Ident(ident("FATAL"))],
                decl: declare_unsigned("err"),
            },
        ],
        default_decl: Some(declare_type("fallback", TypeSpec::Bool)),
    }));

    let json = union.get_type_json();
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("JSON output should be valid for embedded union");

    assert_eq!(parsed["type"], "union");
    assert_eq!(parsed["discriminant"]["name"], "which");
    assert_eq!(parsed["discriminant"]["element_type"]["type"], "int");

    let cases = parsed["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 2);
    assert_eq!(cases[0]["values"][0], "RECOVERABLE");
    assert_eq!(cases[0]["element_type"]["type"], "bool");
    assert_eq!(cases[1]["values"][0], "FATAL");
    assert_eq!(cases[1]["element_type"]["type"], "unsigned");

    let default = parsed["default"].as_object().unwrap();
    assert_eq!(default["name"], "fallback");
    assert_eq!(default["element_type"]["type"], "bool");
}
