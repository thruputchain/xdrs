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
        let field0_json = body.0[0].get_type_json();
        let field1_json = body.0[1].get_type_json();
        let field2_json = body.0[2].get_type_json();

        println!("Field 0 JSON: {}", field0_json);
        println!("Field 1 JSON: {}", field1_json);
        println!("Field 2 JSON: {}", field2_json);

        // Fields should have the correct types
        assert!(field0_json.contains("int"));
        assert!(field1_json.contains("unsigned"));
        assert!(field2_json.contains("bool"));
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
