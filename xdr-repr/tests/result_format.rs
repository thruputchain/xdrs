use xdr_repr::*;

#[test]
fn raw_embedded_union_formats_with_void_arms() {
    assert_eq!(
        TypeSpec::Union(std::boxed::Box::new(UnionBody {
            discriminant_decl: declare_int("which"),
            case_specs: vec![
                CaseSpec {
                    cases: vec![signed(0)],
                    decl: declare_void(),
                },
                CaseSpec {
                    cases: vec![signed(1)],
                    decl: declare_void(),
                },
            ],
            default_decl: None,
        }))
        .to_string(),
        "union switch (int which)\n{\ncase 0:\n\tvoid;\ncase 1:\n\tvoid;\n}"
    );
}

#[test]
fn primitive_types_format_without_panicking() {
    let cases = [
        (TypeSpec::Int, "int"),
        (TypeSpec::Unsigned, "unsigned int"),
        (TypeSpec::Hyper, "hyper"),
        (TypeSpec::UnsignedHyper, "unsigned hyper"),
        (TypeSpec::Float, "float"),
        (TypeSpec::Double, "double"),
        (TypeSpec::Quadruple, "quadruple"),
        (TypeSpec::Bool, "bool"),
    ];

    for (ty, expected) in cases {
        assert_eq!(ty.to_string(), expected);
    }
}
