use xdr_repr::*;

// Tests for TypeSpec::get_type_json()

#[test]
fn typespec_int_json() {
    assert_eq!(TypeSpec::Int.get_type_json(), "int");
}

#[test]
fn typespec_unsigned_json() {
    assert_eq!(TypeSpec::Unsigned.get_type_json(), "unsigned");
}

#[test]
fn typespec_hyper_json() {
    assert_eq!(TypeSpec::Hyper.get_type_json(), "hyper");
}

#[test]
fn typespec_unsigned_hyper_json() {
    assert_eq!(TypeSpec::UnsignedHyper.get_type_json(), "unsigned hyper");
}

#[test]
fn typespec_float_json() {
    assert_eq!(TypeSpec::Float.get_type_json(), "float");
}

#[test]
fn typespec_double_json() {
    assert_eq!(TypeSpec::Double.get_type_json(), "double");
}

#[test]
fn typespec_quadruple_json() {
    assert_eq!(TypeSpec::Quadruple.get_type_json(), "quadruple");
}

#[test]
fn typespec_bool_json() {
    assert_eq!(TypeSpec::Bool.get_type_json(), "bool");
}

#[test]
fn typespec_ident_json() {
    let custom_type = TypeSpec::Ident(ident("CustomType"));
    assert_eq!(custom_type.get_type_json(), "CustomType");
}

// Tests for Declaration::get_type_json()

#[test]
fn void_declaration_json() {
    let void_decl = Declaration::Void;
    let json = void_decl.get_type_json();
    assert_eq!(json, "{ \"type\": \"void\" }");
}

#[test]
fn named_declaration_json() {
    let named_decl = declare_type("value", TypeSpec::Int);
    let json = named_decl.get_type_json();
    assert_eq!(json, "{ \"type\": \"int\" }");
}

#[test]
fn fixed_opaque_declaration_json() {
    let fixed_opaque = declare_fixed_opaque("data", 10);
    let json = fixed_opaque.get_type_json();
    assert_eq!(json, "{ \"type\": \"fixed opaque\", \"len\": 10 }");
}

#[test]
fn variable_opaque_declaration_json() {
    let var_opaque = Declaration::VariableOpaque {
        name: ident("data"),
        max: unsigned(100),
    };
    let json = var_opaque.get_type_json();
    assert_eq!(json, "{ \"type\": \"variable opaque\", \"max\": 100 }");
}

#[test]
fn string_declaration_json() {
    let string_decl = Declaration::String {
        name: ident("name"),
        max: unsigned(50),
    };
    let json = string_decl.get_type_json();
    assert_eq!(json, "{ \"type\": \"string\", \"max\": 50 }");
}

#[test]
fn fixed_array_declaration_json() {
    let fixed_array = Declaration::FixedArray {
        name: ident("items"),
        elt_ty: TypeSpec::Unsigned,
        len: unsigned(5),
    };
    let json = fixed_array.get_type_json();
    assert_eq!(
        json,
        "{ \"type\": \"fixed array\", \"element_type\": { \"type\": \"unsigned\" }, \"len\": 5 }"
    );
}

#[test]
fn variable_array_declaration_json() {
    let var_array = Declaration::VariableArray {
        name: ident("items"),
        elt_ty: TypeSpec::Int,
        max: unsigned(100),
    };
    let json = var_array.get_type_json();
    assert_eq!(
        json,
        "{ \"type\": \"variable array\", \"element_type\": { \"type\": \"int\" }, \"max\": 100 }"
    );
}

#[test]
fn optional_declaration_json() {
    let optional_decl = Declaration::Optional {
        name: ident("value"),
        opt_ty: TypeSpec::Bool,
    };
    let json = optional_decl.get_type_json();
    assert_eq!(
        json,
        "{ \"type\": \"optional\", \"element_type\": { \"type\": \"bool\" } }"
    );
}

// Tests for all primitive types in Named declarations

#[test]
fn named_declaration_all_primitives() {
    // Test all primitive types
    let test_cases = vec![
        (TypeSpec::Int, "{ \"type\": \"int\" }"),
        (TypeSpec::Unsigned, "{ \"type\": \"unsigned\" }"),
        (TypeSpec::Hyper, "{ \"type\": \"hyper\" }"),
        (TypeSpec::UnsignedHyper, "{ \"type\": \"unsigned hyper\" }"),
        (TypeSpec::Float, "{ \"type\": \"float\" }"),
        (TypeSpec::Double, "{ \"type\": \"double\" }"),
        (TypeSpec::Quadruple, "{ \"type\": \"quadruple\" }"),
        (TypeSpec::Bool, "{ \"type\": \"bool\" }"),
    ];

    for (type_spec, expected) in test_cases {
        let decl = declare_type("field", type_spec);
        assert_eq!(decl.get_type_json(), expected);
    }
}

#[test]
fn named_declaration_custom_type() {
    let decl = declare_type("field", TypeSpec::Ident(ident("MyCustomType")));
    assert_eq!(decl.get_type_json(), "{ \"type\": \"MyCustomType\" }");
}

#[test]
fn fixed_array_all_types() {
    // Test fixed arrays with different element types
    let int_array = Declaration::FixedArray {
        name: ident("nums"),
        elt_ty: TypeSpec::Int,
        len: unsigned(10),
    };
    assert_eq!(
        int_array.get_type_json(),
        "{ \"type\": \"fixed array\", \"element_type\": { \"type\": \"int\" }, \"len\": 10 }"
    );

    let hyper_array = Declaration::FixedArray {
        name: ident("big_nums"),
        elt_ty: TypeSpec::Hyper,
        len: unsigned(5),
    };
    assert_eq!(
        hyper_array.get_type_json(),
        "{ \"type\": \"fixed array\", \"element_type\": { \"type\": \"hyper\" }, \"len\": 5 }"
    );

    let custom_array = Declaration::FixedArray {
        name: ident("items"),
        elt_ty: TypeSpec::Ident(ident("CustomType")),
        len: unsigned(3),
    };
    assert_eq!(
        custom_array.get_type_json(),
        "{ \"type\": \"fixed array\", \"element_type\": { \"type\": \"CustomType\" }, \"len\": 3 }"
    );
}

#[test]
fn variable_array_all_types() {
    // Test variable arrays with different element types
    let float_array = Declaration::VariableArray {
        name: ident("floats"),
        elt_ty: TypeSpec::Float,
        max: unsigned(100),
    };
    assert_eq!(
        float_array.get_type_json(),
        "{ \"type\": \"variable array\", \"element_type\": { \"type\": \"float\" }, \"max\": 100 }"
    );

    let custom_array = Declaration::VariableArray {
        name: ident("items"),
        elt_ty: TypeSpec::Ident(ident("Record")),
        max: unsigned(50),
    };
    assert_eq!(
        custom_array.get_type_json(),
        "{ \"type\": \"variable array\", \"element_type\": { \"type\": \"Record\" }, \"max\": 50 }"
    );
}

#[test]
fn optional_all_types() {
    // Test optionals with different types
    let opt_int = Declaration::Optional {
        name: ident("maybe_num"),
        opt_ty: TypeSpec::Int,
    };
    assert_eq!(
        opt_int.get_type_json(),
        "{ \"type\": \"optional\", \"element_type\": { \"type\": \"int\" } }"
    );

    let opt_double = Declaration::Optional {
        name: ident("maybe_float"),
        opt_ty: TypeSpec::Double,
    };
    assert_eq!(
        opt_double.get_type_json(),
        "{ \"type\": \"optional\", \"element_type\": { \"type\": \"double\" } }"
    );

    let opt_custom = Declaration::Optional {
        name: ident("maybe_record"),
        opt_ty: TypeSpec::Ident(ident("MyRecord")),
    };
    assert_eq!(
        opt_custom.get_type_json(),
        "{ \"type\": \"optional\", \"element_type\": { \"type\": \"MyRecord\" } }"
    );
}

// Tests for embedded struct types

#[test]
fn embedded_struct_empty() {
    let empty_struct = TypeSpec::Struct(StructBody(vec![]));
    assert_eq!(
        empty_struct.get_type_json(),
        "{\n  \"type\": \"struct\",\n  \"fields\": []\n}"
    );
}

#[test]
fn embedded_struct_simple() {
    let struct_body = StructBody(vec![
        declare_type("id", TypeSpec::Unsigned),
        declare_type("count", TypeSpec::Int),
    ]);
    let struct_type = TypeSpec::Struct(struct_body);
    let json = struct_type.get_type_json();
    assert_eq!(
        json,
        r#"{
  "type": "struct",
  "fields": [
    {
      "name": "id",
      "element_type": { "type": "unsigned" }
    },
    {
      "name": "count",
      "element_type": { "type": "int" }
    }
  ]
}"#
    );
}

#[test]
fn embedded_struct_complex() {
    let struct_body = StructBody(vec![
        declare_type("name", TypeSpec::Ident(ident("String"))),
        declare_fixed_opaque("data", 16),
        Declaration::FixedArray {
            name: ident("values"),
            elt_ty: TypeSpec::Unsigned,
            len: unsigned(10),
        },
    ]);
    let struct_type = TypeSpec::Struct(struct_body);
    let json = struct_type.get_type_json();
    assert_eq!(
        json,
        r#"{
  "type": "struct",
  "fields": [
    {
      "name": "name",
      "element_type": { "type": "String" }
    },
    {
      "name": "data",
      "element_type": { "type": "fixed opaque", "len": 16 }
    },
    {
      "name": "values",
      "element_type": { "type": "fixed array", "element_type": { "type": "unsigned" }, "len": 10 }
    }
  ]
}"#
    );
}

#[test]
fn embedded_struct_in_declaration() {
    let decl = Declaration::Named {
        name: ident("message"),
        ty: TypeSpec::Struct(StructBody(vec![
            declare_type("id", TypeSpec::Unsigned),
            declare_type("payload", TypeSpec::Ident(ident("Bytes"))),
        ])),
    };
    let json = decl.get_type_json();
    assert_eq!(
        json,
        r#"{
    "type": "struct",
    "fields": [
      {
        "name": "id",
        "element_type": { "type": "unsigned" }
      },
      {
        "name": "payload",
        "element_type": { "type": "Bytes" }
      }
    ]
  }"#
    );
}

#[test]
fn nested_struct() {
    // Test a struct containing another struct
    let inner_struct = TypeSpec::Struct(StructBody(vec![
        declare_type("x", TypeSpec::Int),
        declare_type("y", TypeSpec::Int),
    ]));

    let outer_struct = StructBody(vec![
        declare_type("point", inner_struct),
        declare_type("label", TypeSpec::Ident(ident("String"))),
    ]);

    let json = TypeSpec::Struct(outer_struct).get_type_json();
    assert_eq!(
        json,
        r#"{
  "type": "struct",
  "fields": [
    {
      "name": "point",
      "element_type": {
          "type": "struct",
          "fields": [
            {
              "name": "x",
              "element_type": { "type": "int" }
            },
            {
              "name": "y",
              "element_type": { "type": "int" }
            }
          ]
        }
    },
    {
      "name": "label",
      "element_type": { "type": "String" }
    }
  ]
}"#
    );
}
