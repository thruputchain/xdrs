use xdr_repr::xdr::*;

#[test]
fn parse_spec() {
    let spec: Specification = syn::parse_quote! {

        // Fun stuff!
        const FOO = -1;
        const BAR = 234229;

        enum Result {
            OK = 0,
            ERROR = 1
        };

        struct Pair {
            void;
            void;
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
                    Ident("Pair".to_string()),
                    StructBody(vec![
                        Declaration::Void,
                        Declaration::Void,
                    ]))),
        ]
    };

    // assert_eq!(spec, expected);
    if spec != expected {
        panic!("\nExpected:{START}{expected}{END}\nFound:{START}{spec}{END}\n");
    }
}

const START: &str = "\n/------------------------\\\n";
const END: &str = "\\------------------------/\n";
