use xdr_repr::*;

#[test]
fn default_only_union_formats_valid_idl() {
    let ty = union_def(
        "CatchAll",
        "which",
        TypeSpec::Int,
        vec![],
        Some(declare_void()),
    );

    let idl = ty.to_string();
    assert!(idl.contains("default:"), "IDL must contain default arm");
    assert!(
        !idl.lines()
            .any(|line| line.trim_start().starts_with("case ")),
        "IDL must not contain any case labels"
    );
}

#[test]
fn default_only_union_json_has_empty_cases() {
    let body = UnionBody {
        discriminant_decl: declare_int("which"),
        case_specs: vec![],
        default_decl: Some(declare_void()),
    };

    let json = TypeSpec::Union(Box::new(body)).get_type_json();
    assert!(json.contains("\"cases\": ["), "JSON must have cases array");
    assert!(
        json.contains("\"default\":"),
        "JSON must have default field"
    );
}

#[test]
#[should_panic(expected = "must have at least one case-spec or a default arm")]
fn union_with_no_cases_and_no_default_panics() {
    union_def("Empty", "which", TypeSpec::Int, vec![], None);
}

#[test]
#[should_panic(expected = "every CaseSpec must have at least one discriminant value")]
fn union_with_empty_case_spec_panics() {
    union_def(
        "Bad",
        "which",
        TypeSpec::Int,
        vec![CaseSpec {
            cases: vec![],
            decl: declare_unsigned("value"),
        }],
        None,
    );
}

#[test]
#[should_panic(expected = "duplicate discriminant value")]
fn union_with_duplicate_value_across_arms_panics() {
    union_def(
        "Ambiguous",
        "which",
        TypeSpec::Int,
        vec![
            CaseSpec {
                cases: vec![signed(0)],
                decl: declare_unsigned("amount"),
            },
            CaseSpec {
                cases: vec![signed(0)],
                decl: declare_string("memo", 32),
            },
        ],
        None,
    );
}

#[test]
#[should_panic(expected = "duplicate discriminant value")]
fn union_with_duplicate_value_within_arm_panics() {
    union_def(
        "Ambiguous",
        "which",
        TypeSpec::Int,
        vec![CaseSpec {
            cases: vec![signed(1), signed(1)],
            decl: declare_unsigned("amount"),
        }],
        None,
    );
}

#[test]
fn union_with_distinct_values_across_arms_succeeds() {
    union_def(
        "Good",
        "which",
        TypeSpec::Int,
        vec![
            CaseSpec {
                cases: vec![signed(0)],
                decl: declare_unsigned("amount"),
            },
            CaseSpec {
                cases: vec![signed(1)],
                decl: declare_string("memo", 32),
            },
        ],
        None,
    );
}

#[test]
#[should_panic(expected = "duplicate discriminant value")]
fn union_with_signed_unsigned_same_wire_value_panics() {
    union_def(
        "Ambiguous",
        "which",
        TypeSpec::Int,
        vec![
            CaseSpec {
                cases: vec![signed(0)],
                decl: declare_unsigned("amount"),
            },
            CaseSpec {
                cases: vec![unsigned(0)],
                decl: declare_string("memo", 32),
            },
        ],
        None,
    );
}

#[test]
fn union_with_multi_value_arm_distinct_succeeds() {
    union_def(
        "Good",
        "which",
        TypeSpec::Int,
        vec![CaseSpec {
            cases: vec![signed(0), signed(1), signed(2)],
            decl: declare_unsigned("amount"),
        }],
        None,
    );
}
