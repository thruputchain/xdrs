use xdr_repr::*;

#[test]
fn result_unit_unit_type_spec_is_inline_union() {
    let type_spec = <Result<(), ()>>::type_spec();
    let TypeSpec::Union(body) = type_spec else {
        panic!("Expected TypeSpec::Union");
    };

    assert_eq!(body.discriminant_decl, declare_int("which"));
    assert_eq!(body.case_specs.len(), 2);
    assert_eq!(body.case_specs[0].cases, vec![signed(0)]);
    assert!(matches!(body.case_specs[0].decl, Declaration::Void));
    assert_eq!(body.case_specs[1].cases, vec![signed(1)]);
    assert!(matches!(body.case_specs[1].decl, Declaration::Void));
    assert!(body.default_decl.is_none());
}

#[test]
fn result_bool_u32_type_spec_uses_ok_and_err_cases() {
    let type_spec = <Result<bool, u32>>::type_spec();
    let TypeSpec::Union(body) = type_spec else {
        panic!("Expected TypeSpec::Union");
    };

    assert_eq!(body.discriminant_decl, declare_int("which"));
    assert_eq!(body.case_specs.len(), 2);
    assert_eq!(body.case_specs[0].cases, vec![signed(0)]);
    assert_eq!(body.case_specs[0].decl, declare_type("ok", TypeSpec::Bool));
    assert_eq!(body.case_specs[1].cases, vec![signed(1)]);
    assert_eq!(body.case_specs[1].decl, declare_unsigned("err"));
    assert!(body.default_decl.is_none());
}

#[test]
fn result_bool_u32_formats_as_inline_union() {
    assert_eq!(
        <Result<bool, u32>>::type_spec().to_string(),
        "union switch (int which)\n{\ncase 0:\n\tbool ok;\ncase 1:\n\tunsigned int err;\n}"
    );
}
