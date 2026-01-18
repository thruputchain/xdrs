use xdr_repr::*;
use xdrs::*;

// Tests for JSON spec generation with tuple types
// Tuples are mapped to XDR structs with numbered fields (0, 1, 2, ...)
// and support sizes 1-16 (matching ReadXDR/WriteXDR implementations)

// First, let's verify that primitive tuples work for basic operations
#[test]
fn tuple_2_declare() {
    // Test 2-tuple declaration
    let decl = <(i32, i32)>::declare(ident("point"));
    let json = decl.get_type_json();

    // Tuples should embed the struct definition inline
    assert_eq!(
        json,
        r#"{
  "type": {
    "type": "struct",
    "fields": [
      {
        "name": "0",
        "field_type": { "type": "int" }
      },
      {
        "name": "1",
        "field_type": { "type": "int" }
      }
    ]
  }
}"#
    );
}

#[test]
fn tuple_3_declare() {
    // Test 3-tuple with different types
    let decl = <(u32, bool, i64)>::declare(ident("data"));
    let json = decl.get_type_json();

    // Should have embedded struct with 3 fields
    assert!(json.contains(r#""type": "struct""#));
    assert!(json.contains(r#""name": "0""#));
    assert!(json.contains(r#""name": "1""#));
    assert!(json.contains(r#""name": "2""#));
    assert!(json.contains(r#""type": "unsigned""#));
    assert!(json.contains(r#""type": "bool""#));
    assert!(json.contains(r#""type": "hyper""#));
}

#[test]
fn tuple_type_spec() {
    // Test TypeSpec for a tuple
    let type_spec = <(i32, i32)>::type_spec();

    // Should be TypeSpec::Struct
    match type_spec {
        TypeSpec::Struct(body) => {
            assert_eq!(body.0.len(), 2);
        }
        _ => panic!("Expected TypeSpec::Struct"),
    }
}

#[test]
fn tuple_in_struct_field() {
    // Test using a tuple in a struct field through derive
    #[derive(XDR)]
    struct Point {
        coords: (i32, i32),
    }

    let type_def = Point::type_def();
    if let TypeDef::Struct(_, body) = type_def {
        assert_eq!(body.0.len(), 1);

        // Get JSON for the tuple field - should be an embedded struct
        let coords_json = body.0[0].get_type_json();

        // Verify the JSON has the embedded struct
        assert!(coords_json.contains(r#""type": "struct""#));
        assert!(coords_json.contains(r#""name": "0""#));
        assert!(coords_json.contains(r#""name": "1""#));
    } else {
        panic!("Expected TypeDef::Struct");
    }
}

#[test]
fn tuple_various_sizes() {
    // Test different tuple sizes (1 through 5)
    // All should have embedded struct definitions

    let json1 = <(i32,)>::declare(ident("single")).get_type_json();
    assert!(json1.contains(r#""type": "struct""#));

    let json2 = <(i32, u32)>::declare(ident("pair")).get_type_json();
    assert!(json2.contains(r#""type": "struct""#));

    let json3 = <(i32, u32, bool)>::declare(ident("triple")).get_type_json();
    assert!(json3.contains(r#""type": "struct""#));

    let json4 = <(i32, u32, bool, i64)>::declare(ident("quad")).get_type_json();
    assert!(json4.contains(r#""type": "struct""#));

    let json5 = <(i32, u32, bool, i64, u64)>::declare(ident("quint")).get_type_json();
    assert!(json5.contains(r#""type": "struct""#));
}

#[test]
fn nested_tuple() {
    // Test nested tuples - should have fully embedded nested structures
    let decl = <((i32, i32), (u32, u32))>::declare(ident("nested"));
    let json = decl.get_type_json();

    // Should have nested struct definitions
    assert!(json.contains(r#""type": "struct""#));
    // The outer struct should have 2 fields, each being a nested struct
    assert!(json.contains(r#""name": "0""#));
    assert!(json.contains(r#""name": "1""#));
}

#[test]
fn deeply_nested_tuple() {
    // Test complex nested tuple: (i32, (u32, (i64, u64), bool))
    let decl = <(i32, (u32, (i64, u64), bool))>::declare(ident("deeply_nested"));
    let json = decl.get_type_json();

    // Should have deeply nested struct definitions
    assert!(json.contains(r#""type": "struct""#));
    assert!(json.contains(r#""name": "0""#));
    assert!(json.contains(r#""name": "1""#));
}

#[test]
fn deeply_nested_tuple_structure() {
    // Test deeply nested tuple: (i32, (u32, (i64, u64), bool))
    // This should now have the full struct embedded inline
    let type_spec = <(i32, (u32, (i64, u64), bool))>::type_spec();
    let json = type_spec.get_type_json();

    // The full nested structure should be embedded
    assert_eq!(
        json,
        r#"{
  "type": "struct",
  "fields": [
    {
      "name": "0",
      "field_type": { "type": "int" }
    },
    {
      "name": "1",
      "field_type": {
        "type": {
          "type": "struct",
          "fields": [
            {
              "name": "0",
              "field_type": { "type": "unsigned" }
            },
            {
              "name": "1",
              "field_type": {
                "type": {
                  "type": "struct",
                  "fields": [
                    {
                      "name": "0",
                      "field_type": { "type": "hyper" }
                    },
                    {
                      "name": "1",
                      "field_type": { "type": "unsigned hyper" }
                    }
                  ]
                }
              }
            },
            {
              "name": "2",
              "field_type": { "type": "bool" }
            }
          ]
        }
      }
    }
  ]
}"#
    );
}

#[test]
fn tuple_with_all_primitives() {
    // Test tuple with various primitive types
    let decl = <(i32, u32, i64, u64, bool)>::declare(ident("all_types"));
    let json = decl.get_type_json();

    // Should have embedded struct with all primitive types
    assert!(json.contains(r#""type": "struct""#));
    assert!(json.contains(r#""type": "int""#));
    assert!(json.contains(r#""type": "unsigned""#));
    assert!(json.contains(r#""type": "hyper""#));
    assert!(json.contains(r#""type": "unsigned hyper""#));
    assert!(json.contains(r#""type": "bool""#));
}

#[test]
fn tuple_type_spec_structure() {
    // Test that tuples return embedded structs with numbered fields
    let type_spec = <(i32, u32, bool)>::type_spec();

    // Tuples should return TypeSpec::Struct (embedded struct)
    if let TypeSpec::Struct(body) = type_spec {
        // Should have 3 fields (0, 1, 2)
        assert_eq!(body.0.len(), 3);

        // Check field types
        assert_eq!(body.0[0].get_type_json(), "{ \"type\": \"int\" }");
        assert_eq!(body.0[1].get_type_json(), "{ \"type\": \"unsigned\" }");
        assert_eq!(body.0[2].get_type_json(), "{ \"type\": \"bool\" }");

        // Check the full struct JSON representation
        let struct_json = TypeSpec::Struct(body).get_type_json();
        assert_eq!(
            struct_json,
            r#"{
  "type": "struct",
  "fields": [
    {
      "name": "0",
      "field_type": { "type": "int" }
    },
    {
      "name": "1",
      "field_type": { "type": "unsigned" }
    },
    {
      "name": "2",
      "field_type": { "type": "bool" }
    }
  ]
}"#
        );
    } else {
        panic!("Expected TypeSpec::Struct for tuple");
    }
}

#[test]
fn tuple_typespec_json() {
    // Test the JSON output of tuple as TypeSpec
    let type_spec = <(i32, i32)>::type_spec();
    let json = type_spec.get_type_json();

    // TypeSpec for tuples should be an embedded struct
    assert_eq!(
        json,
        r#"{
  "type": "struct",
  "fields": [
    {
      "name": "0",
      "field_type": { "type": "int" }
    },
    {
      "name": "1",
      "field_type": { "type": "int" }
    }
  ]
}"#
    );
}

#[test]
fn nested_tuple_in_derived_struct() {
    // Test using a complex nested tuple in a derived struct
    #[derive(XDR)]
    struct ComplexNesting {
        data: (i32, (u32, (i64, u64), bool)),
    }

    let type_def = ComplexNesting::type_def();

    if let TypeDef::Struct(name, body) = type_def {
        assert_eq!(name.0, "ComplexNesting");
        assert_eq!(body.0.len(), 1);

        // The field should have the embedded struct inline
        let json = body.0[0].get_type_json();

        // Verify it contains the fully nested structure
        assert!(json.contains(r#""type": "struct""#));
        assert!(json.contains(r#""name": "0""#));
        assert!(json.contains(r#""name": "1""#));
        assert!(json.contains(r#""type": "int""#));
        assert!(json.contains(r#""type": "unsigned""#));
        assert!(json.contains(r#""type": "hyper""#));
        assert!(json.contains(r#""type": "unsigned hyper""#));
        assert!(json.contains(r#""type": "bool""#));
    } else {
        panic!("Expected TypeDef::Struct");
    }
}

#[test]
fn tuple_in_declaration_xdr() {
    // Test XDR output when tuple is used in a declaration
    let decl = <(i32, u32, bool)>::declare(ident("my_tuple"));

    // The declaration should have an embedded struct
    let decl_str = format!("{}", decl);

    // Should show the embedded struct inline
    assert!(decl_str.contains("struct"));
    assert!(decl_str.contains("int 0;"));
    assert!(decl_str.contains("unsigned int 1;"));
    assert!(decl_str.contains("bool 2;"));
}

#[test]
fn nested_tuple_in_declaration_xdr() {
    // Test nested tuple in declaration XDR output
    let decl = <(i32, (u32, bool))>::declare(ident("nested"));
    let decl_str = format!("{}", decl);

    // Should show nested struct definitions
    assert!(decl_str.contains("struct"));
    assert!(decl_str.contains("int 0;"));
}
