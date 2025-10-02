use xdr_repr::xdr::*;

#[test]
fn parse_spec() {
    let spec: Specification = syn::parse_quote! {

        // Fun stuff!
        const FOO = -1;
        const BAR = 234229;

    };

    let expected = Specification {
        definitions: vec![
            Definition::Const(ConstDef {
                ident: Ident("FOO".to_string()),
                constant: Constant::Signed(-1),
            }),
            Definition::Const(ConstDef {
                ident: Ident("BAR".to_string()),
                constant: Constant::Unsigned(234229),
            }),
        ]
    };

    assert_eq!(spec, expected);
}
