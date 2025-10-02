use xdr_repr::xdr::*;

#[test]
fn parse_constant_def() {
    let cd: ConstDef = syn::parse_quote! {
        const FOO = -1;
    };

    let expected = ConstDef {
        ident: Ident("FOO".to_string()),
        constant: Constant::Signed(-1),
    };

    assert_eq!(cd, expected);
}
