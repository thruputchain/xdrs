use xdr_repr::*;
use xdrs::*;

// Tests for JSON spec generation with tuple types
// Tuples are mapped to XDR structs with numbered fields (0, 1, 2, ...)
// and support sizes 1-16 (matching ReadXDR/WriteXDR implementations)

// First, let's verify that primitive tuples work for basic operations
#[test]
fn tuple_2_declare() {
    // Test 2-tuple
    let decl = <(i32, i32)>::declare(ident("point"));
    let json = decl.get_type_json();

    // Tuples map to named types (since they implement HasTypeDef)
    // They should produce a simple type reference
    println!("2-tuple JSON: {}", json);

    // Should contain "Tuple" as the type name
    assert!(json.contains("Tuple"));
    assert!(!json.is_empty());
}

#[test]
fn tuple_3_declare() {
    // Test 3-tuple with different types
    let decl = <(u32, bool, i64)>::declare(ident("data"));
    let json = decl.get_type_json();

    println!("3-tuple JSON: {}", json);
    assert!(!json.is_empty());
}

#[test]
fn tuple_type_spec() {
    // Test TypeSpec for a tuple
    let type_spec = <(i32, i32)>::type_spec();
    let json = type_spec.get_type_json();

    println!("Tuple TypeSpec JSON: {}", json);
    assert!(!json.is_empty());
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

        // Get JSON for the tuple field
        let coords_json = body.0[0].get_type_json();
        println!("Tuple field JSON: {}", coords_json);

        // Verify the JSON represents the tuple type
        assert!(!coords_json.is_empty());
    } else {
        panic!("Expected TypeDef::Struct");
    }
}

#[test]
fn tuple_various_sizes() {
    // Test different tuple sizes (1 through 5)

    // 1-tuple
    let decl1 = <(i32,)>::declare(ident("single"));
    let json1 = decl1.get_type_json();
    println!("1-tuple JSON: {}", json1);
    assert!(!json1.is_empty());

    // 2-tuple
    let decl2 = <(i32, u32)>::declare(ident("pair"));
    let json2 = decl2.get_type_json();
    println!("2-tuple JSON: {}", json2);
    assert!(!json2.is_empty());

    // 3-tuple
    let decl3 = <(i32, u32, bool)>::declare(ident("triple"));
    let json3 = decl3.get_type_json();
    println!("3-tuple JSON: {}", json3);
    assert!(!json3.is_empty());

    // 4-tuple
    let decl4 = <(i32, u32, bool, i64)>::declare(ident("quad"));
    let json4 = decl4.get_type_json();
    println!("4-tuple JSON: {}", json4);
    assert!(!json4.is_empty());

    // 5-tuple
    let decl5 = <(i32, u32, bool, i64, u64)>::declare(ident("quint"));
    let json5 = decl5.get_type_json();
    println!("5-tuple JSON: {}", json5);
    assert!(!json5.is_empty());
}

#[test]
fn nested_tuple() {
    // Test nested tuples
    let decl = <((i32, i32), (u32, u32))>::declare(ident("nested"));
    let json = decl.get_type_json();

    println!("Nested tuple JSON: {}", json);
    assert!(!json.is_empty());
}

#[test]
fn deeply_nested_tuple() {
    // Test complex nested tuple: (A, (B, (C, D), E))
    // Using concrete types: (i32, (u32, (i64, u64), bool))
    let decl = <(i32, (u32, (i64, u64), bool))>::declare(ident("deeply_nested"));
    let json = decl.get_type_json();

    println!("Deeply nested tuple JSON: {}", json);

    // The outer tuple should be a Tuple type
    assert!(json.contains("Tuple"));
    assert!(!json.is_empty());
}

#[test]
fn deeply_nested_tuple_structure() {
    // Examine the structure in detail: (i32, (u32, (i64, u64), bool))
    let type_def = <(i32, (u32, (i64, u64), bool))>::type_def();

    if let TypeDef::Struct(name, body) = type_def {
        assert_eq!(name.0, "Tuple");
        assert_eq!(body.0.len(), 2, "Outer tuple should have 2 fields");

        // Check full JSON for outer tuple struct
        let struct_json = TypeSpec::Struct(body.clone()).get_type_json();
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
      "field_type": { "type": "Tuple" }
    }
  ]
}"#
        );

        // Check middle tuple: (u32, (i64, u64), bool)
        let middle_def = <(u32, (i64, u64), bool)>::type_def();
        if let TypeDef::Struct(mid_name, mid_body) = middle_def {
            assert_eq!(mid_name.0, "Tuple");
            assert_eq!(mid_body.0.len(), 3);

            let mid_struct_json = TypeSpec::Struct(mid_body).get_type_json();
            assert_eq!(
                mid_struct_json,
                r#"{
  "type": "struct",
  "fields": [
    {
      "name": "0",
      "field_type": { "type": "unsigned" }
    },
    {
      "name": "1",
      "field_type": { "type": "Tuple" }
    },
    {
      "name": "2",
      "field_type": { "type": "bool" }
    }
  ]
}"#
            );
        }

        // Check innermost tuple: (i64, u64)
        let inner_def = <(i64, u64)>::type_def();
        if let TypeDef::Struct(inner_name, inner_body) = inner_def {
            assert_eq!(inner_name.0, "Tuple");
            assert_eq!(inner_body.0.len(), 2);

            let inner_struct_json = TypeSpec::Struct(inner_body).get_type_json();
            assert_eq!(
                inner_struct_json,
                r#"{
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
}"#
            );
        }
    } else {
        panic!("Expected TypeDef::Struct for outer tuple");
    }
}

#[test]
fn tuple_with_all_primitives() {
    // Test tuple with various primitive types (i32, u32, i64, u64, bool)
    let decl = <(i32, u32, i64, u64, bool)>::declare(ident("all_types"));
    let json = decl.get_type_json();

    println!("All primitives tuple JSON: {}", json);
    assert!(!json.is_empty());

    // Verify it contains type information
    assert!(json.contains("type") || json.contains("struct") || json.contains("Tuple"));
}

#[test]
fn tuple_type_def_structure() {
    // Test that tuples map to structs with numbered fields
    let type_def = <(i32, u32, bool)>::type_def();

    if let TypeDef::Struct(name, body) = type_def {
        // Name should be "Tuple"
        assert_eq!(name.0, "Tuple");

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
        panic!("Expected TypeDef::Struct for tuple");
    }
}

#[test]
fn tuple_typespec_json() {
    // Test the JSON output of tuple as TypeSpec
    let type_spec = <(i32, i32)>::type_spec();
    let json = type_spec.get_type_json();

    println!("Tuple TypeSpec JSON: {}", json);

    // TypeSpec for tuples should be an identifier "Tuple"
    assert_eq!(json, "Tuple");
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

        // The field should reference the Tuple type
        assert_eq!(body.0[0].get_type_json(), "{ \"type\": \"Tuple\" }");
    } else {
        panic!("Expected TypeDef::Struct");
    }
}

#[test]
fn tuple_xdr_def_output() {
    // Test the XDR definition format output for a tuple
    let def = <(i32, u32, bool)>::def();
    let def_str = format!("{}", def);

    println!("\n=== Tuple (i32, u32, bool) XDR Def ===\n{}", def_str);

    // Should be a struct Tuple with numbered fields
    assert_eq!(
        def_str,
        "struct Tuple {\n\tint 0;\n\tunsigned int 1;\n\tbool 2;\n};\n"
    );
}

#[test]
fn nested_tuple_xdr_def_output() {
    // Test nested tuple XDR def
    let def = <(i32, (u32, bool))>::def();
    let def_str = format!("{}", def);

    println!("\n=== Nested Tuple (i32, (u32, bool)) XDR Def ===\n{}", def_str);

    // Outer tuple should reference inner Tuple type
    assert_eq!(
        def_str,
        "struct Tuple {\n\tint 0;\n\tTuple 1;\n};\n"
    );
}
