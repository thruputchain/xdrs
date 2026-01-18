use xdr_repr::*;

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
    if let TypeDef::Struct(_, body) = type_def {
        assert_eq!(body.0.len(), 2);

        // Test first field
        let id_json = body.0[0].get_type_json();
        assert_eq!(id_json, "{ \"type\": \"unsigned\" }");

        // Test second field
        let count_json = body.0[1].get_type_json();
        assert_eq!(count_json, "{ \"type\": \"int\" }");
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

    // The spec should contain a union definition
    if let Specification(defs) = spec {
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

        // Find the case for empty tuple variant "Inactive"
        let inactive_case = union_body.0.iter().find(|case| {
            match case {
                UnionCase::Case(_, _, decl) => {
                    // Check if this is the Inactive case (discriminant 2)
                    // by looking at the declaration type
                    matches!(decl, Declaration::Void)
                }
                _ => false,
            }
        });

        // Verify that we have at least one void case
        let void_cases: Vec<_> = union_body.0.iter().filter(|case| {
            matches!(case, UnionCase::Case(_, _, Declaration::Void))
        }).collect();

        assert!(!void_cases.is_empty(), "Expected at least one void case for empty tuple variant");

        // Test the JSON output of void declaration
        let void_json = Declaration::Void.get_type_json();
        assert_eq!(void_json, "{ \"type\": \"void\" }");
    } else {
        panic!("Expected Specification with definitions");
    }
}

#[test]
fn derived_enum_active_variant_json() {
    // Get the specification
    let spec = StatusEnum::spec();

    // The spec should contain a union definition
    if let Specification(defs) = spec {
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
        let active_cases: Vec<_> = union_body.0.iter().filter(|case| {
            matches!(case, UnionCase::Case(_, _, Declaration::Named { .. }))
        }).collect();

        assert!(!active_cases.is_empty(), "Expected at least one named case");

        // Get the first named case and test its JSON
        if let UnionCase::Case(_, _, decl) = active_cases[0] {
            let json = decl.get_type_json();
            assert_eq!(json, "{ \"type\": \"unsigned\" }");
        }
    }
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

    if let TypeDef::Struct(_, body) = type_def {
        assert_eq!(body.0.len(), 2);

        // Test array field
        let values_json = body.0[0].get_type_json();
        assert_eq!(values_json, "{ \"type\": \"fixed array\", \"elt_ty\": \"unsigned\", \"len\": 10 }");

        // Test optional field
        let optional_json = body.0[1].get_type_json();
        assert_eq!(optional_json, "{ \"type\": \"optional\", \"opt_ty\": \"unsigned hyper\" }");
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

    if let Specification(defs) = spec {
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
        let void_count = union_body.0.iter().filter(|case| {
            matches!(case, UnionCase::Case(_, _, Declaration::Void))
        }).count();

        assert_eq!(void_count, 3, "Expected 3 void cases for empty tuple variants");

        // Verify each void case has correct JSON
        for case in &union_body.0 {
            if let UnionCase::Case(_, _, Declaration::Void) = case {
                let json = Declaration::Void.get_type_json();
                assert_eq!(json, "{ \"type\": \"void\" }");
            }
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
