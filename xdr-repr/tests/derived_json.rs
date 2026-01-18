use xdr_repr::*;
use xdrs::*;

// Tests for JSON spec generation with derived types

// Test struct with derived XDR
#[derive(XDR)]
pub struct SimpleStruct {
    id: u32,
    count: i32,
}

#[test]
fn derived_struct_fields_json() {
    // Get the type definition
    let type_def = SimpleStruct::type_def();

    // Extract struct body
    if let TypeDef::Struct(name, body) = type_def {
        assert_eq!(name.0, "SimpleStruct");
        assert_eq!(body.0.len(), 2);

        // Test individual field JSON
        assert_eq!(body.0[0].get_type_json(), "{ \"type\": \"unsigned\" }");
        assert_eq!(body.0[1].get_type_json(), "{ \"type\": \"int\" }");

        // Test full struct JSON
        let struct_json = TypeSpec::Struct(body).get_type_json();
        assert_eq!(
            struct_json,
            r#"{
  "type": "struct",
  "fields": [
    {
      "name": "id",
      "field_type": { "type": "unsigned" }
    },
    {
      "name": "count",
      "field_type": { "type": "int" }
    }
  ]
}"#
        );
    } else {
        panic!("Expected TypeDef::Struct");
    }
}

// Test enum with empty tuple variants (maps to void in union)
#[derive(XDR)]
#[repr(i32)]
pub enum StatusEnum {
    Active { session_id: u32 } = 1,
    Inactive() = 2,           // Empty tuple variant
    Pending {} = 3,           // Empty struct variant
    Error = 4,                // Unit variant
}

#[test]
fn derived_enum_empty_tuple_variant_json() {
    // Get the specification
    let spec = StatusEnum::spec();
    let Specification { definitions: defs } = spec;

    // Find the union definition
    let union_def = defs.iter().find_map(|def| {
        if let Definition::Type(TypeDef::Union(_, union_body)) = def {
            Some(union_body)
        } else {
            None
        }
    });

    assert!(union_def.is_some(), "Expected union definition");
    let union_body = union_def.unwrap();

    // Count void cases (should be 3: Inactive, Pending, Error)
    let void_count = union_body.case_specs.iter().filter(|case_spec| {
        matches!(case_spec.decl, Declaration::Void)
    }).count();

    assert_eq!(void_count, 3, "Expected 3 void cases for empty variants");

    // Test the JSON output of void declaration
    let void_json = Declaration::Void.get_type_json();
    assert_eq!(void_json, "{ \"type\": \"void\" }");
}

#[test]
fn derived_enum_active_variant_json() {
    // Get the specification
    let spec = StatusEnum::spec();
    let Specification { definitions: defs } = spec;

    // Find the union definition
    let union_def = defs.iter().find_map(|def| {
        if let Definition::Type(TypeDef::Union(_, union_body)) = def {
            Some(union_body)
        } else {
            None
        }
    });

    assert!(union_def.is_some(), "Expected union definition");
    let union_body = union_def.unwrap();

    // Find the case with a field (Active variant with session_id)
    let active_cases: Vec<_> = union_body.case_specs.iter().filter(|case_spec| {
        matches!(case_spec.decl, Declaration::Named { .. })
    }).collect();

    assert!(!active_cases.is_empty(), "Expected at least one named case");

    // Get the first named case and test its JSON
    let json = active_cases[0].decl.get_type_json();
    assert_eq!(json, "{ \"type\": \"unsigned\" }");
}

// Test a more complex struct with arrays and optionals
#[derive(XDR)]
pub struct ComplexStruct {
    values: [u32; 10],
    optional_id: Option<u64>,
}

#[test]
fn derived_complex_struct_json() {
    let type_def = ComplexStruct::type_def();

    if let TypeDef::Struct(name, body) = type_def {
        assert_eq!(name.0, "ComplexStruct");
        assert_eq!(body.0.len(), 2);

        // Test individual field JSON
        assert_eq!(body.0[0].get_type_json(), "{ \"type\": \"fixed array\", \"elt_ty\": \"unsigned\", \"len\": 10 }");
        assert_eq!(body.0[1].get_type_json(), "{ \"type\": \"optional\", \"opt_ty\": \"unsigned hyper\" }");

        // Test full struct JSON
        let struct_json = TypeSpec::Struct(body).get_type_json();
        assert_eq!(
            struct_json,
            r#"{
  "type": "struct",
  "fields": [
    {
      "name": "values",
      "field_type": { "type": "fixed array", "elt_ty": "unsigned", "len": 10 }
    },
    {
      "name": "optional_id",
      "field_type": { "type": "optional", "opt_ty": "unsigned hyper" }
    }
  ]
}"#
        );
    } else {
        panic!("Expected TypeDef::Struct");
    }
}

// Test nested enum with multiple tuple variants
#[derive(XDR)]
#[repr(i32)]
pub enum MultiTupleEnum {
    First() = 1,
    Second() = 2,
    Third() = 3,
    Fourth { data: i32 } = 4,
}

#[test]
fn derived_enum_multiple_tuple_variants_json() {
    let spec = MultiTupleEnum::spec();
    let Specification { definitions: defs } = spec;

    let union_def = defs.iter().find_map(|def| {
        if let Definition::Type(TypeDef::Union(_, union_body)) = def {
            Some(union_body)
        } else {
            None
        }
    });

    assert!(union_def.is_some());
    let union_body = union_def.unwrap();

    // Count void cases (should be 3: First, Second, Third)
    let void_count = union_body.case_specs.iter().filter(|case_spec| {
        matches!(case_spec.decl, Declaration::Void)
    }).count();

    assert_eq!(void_count, 3, "Expected 3 void cases for empty tuple variants");

    // Verify each void case has correct JSON
    for case_spec in &union_body.case_specs {
        if matches!(case_spec.decl, Declaration::Void) {
            let json = case_spec.decl.get_type_json();
            assert_eq!(json, "{ \"type\": \"void\" }");
        }
    }
}

// Test that derived types can be used in declarations
#[test]
fn derived_type_in_declaration_json() {
    // Create a declaration that references a derived type
    let decl = Declaration::Named {
        name: ident("status"),
        ty: StatusEnum::type_spec(),
    };

    // The type spec should be an identifier
    let json = decl.get_type_json();
    assert_eq!(json, "{ \"type\": \"StatusEnum\" }");
}

// Test derived struct as TypeSpec
#[test]
fn derived_struct_as_typespec_json() {
    let type_spec = SimpleStruct::type_spec();
    let json = type_spec.get_type_json();
    assert_eq!(json, "SimpleStruct");
}

#[test]
fn derived_struct_xdr_def() {
    // Test XDR spec format output for derived struct
    let def = SimpleStruct::def();
    let def_str = format!("{}", def);

    assert_eq!(
        def_str,
        "struct SimpleStruct {\n\tunsigned int id;\n\tint count;\n};\n"
    );
}

#[test]
fn derived_enum_xdr_spec() {
    // Test XDR spec format for enum with empty tuple variants
    let spec = StatusEnum::spec();
    let spec_str = format!("{}", spec);

    println!("\n=== StatusEnum XDR Spec ===\n{}", spec_str);

    // Should contain the enum type definition
    assert!(spec_str.contains("enum StatusEnumType"));
    assert!(spec_str.contains("Active = 1"));
    assert!(spec_str.contains("Inactive = 2"));
    assert!(spec_str.contains("Pending = 3"));
    assert!(spec_str.contains("Error = 4"));

    // Should contain the union with void cases for empty variants
    assert!(spec_str.contains("union StatusEnum switch"));

    // Check for void cases (empty tuple/struct variants)
    assert!(spec_str.contains("void;"));
}

#[test]
fn derived_complex_struct_xdr_def() {
    let def = ComplexStruct::def();
    let def_str = format!("{}", def);

    assert_eq!(
        def_str,
        "struct ComplexStruct {\n\tunsigned int values[10];\n\tunsigned hyper *optional_id;\n};\n"
    );
}
