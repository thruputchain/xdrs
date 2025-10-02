use xdr_repr::xdr::*;

#[test]
fn parse_type_spec() {
    let parsed: TypeSpec = syn::parse_quote! { int };
    let expected = TypeSpec::Int;

    assert_eq!(parsed, expected);
}

#[test]
fn parse_declaration() {
    let parsed: Declaration = syn::parse_quote! {
        int bar
    };
    let expected = Declaration::Named {
        name: Ident("bar".to_string()),
        ty: TypeSpec::Int
    };

    assert_eq!(parsed, expected);
}

#[test]
fn parse_spec() {
    let parsed: Specification = syn::parse_quote! {

        // Fun stuff!
        const FOO = -1;
        const BAR = 234229;

        enum Result {
            OK = 0,
            ERROR = 1,
        };

        struct NotMuch {
            void;
            void
        };

        struct Foo {
            int bar;
            int baz[3];
            int quux<4>;
            opaque buf[3];
        };
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
            Definition::Type(TypeDef::Enum(
                    Ident("Result".to_string()),
                    EnumBody(vec![
                        (Ident("OK".to_string()), Value::Const(Constant::Unsigned(0))),
                        (Ident("ERROR".to_string()), Value::Const(Constant::Unsigned(1))),
                    ]))),
            Definition::Type(TypeDef::Struct(
                    Ident("NotMuch".to_string()),
                    StructBody(vec![
                        Declaration::Void,
                        Declaration::Void,
                    ]))),
            Definition::Type(TypeDef::Struct(
                    Ident("Foo".to_string()),
                    StructBody(vec![
                        Declaration::Named {
                            name: Ident("bar".to_string()),
                            ty: TypeSpec::Int
                        },
                        Declaration::FixedArray {
                            name: Ident("baz".to_string()),
                            elt_ty: TypeSpec::Int,
                            len: Value::Const(Constant::Unsigned(3)),
                        },
                        Declaration::VariableArray {
                            name: Ident("quux".to_string()),
                            elt_ty: TypeSpec::Int,
                            max: Value::Const(Constant::Unsigned(4)),
                        },
                        Declaration::FixedOpaque {
                            name: Ident("buf".to_string()),
                            size: Value::Const(Constant::Unsigned(3)),
                        },
                    ]))),
        ]
    };

    // assert_eq!(spec, expected);
    if parsed != expected {
        panic!("\nExpected:{START}{expected}{END}\n\
                Parsed:{START}{parsed}{END}\n");
    }
}

const START: &str = "\n/------------------------\\\n";
const END: &str = "\\------------------------/\n";
