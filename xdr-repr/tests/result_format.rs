use xdr_repr::*;

check_format! { raw_embedded_union_formats_with_void_arms
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
    =>
        "union switch (int which)"
        "{"
        "case 0:"
        "\tvoid;"
        "case 1:"
        "\tvoid;"
        "}"
}
