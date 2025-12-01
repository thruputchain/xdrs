use xdr_repr::xdr::*;
use quote::quote;

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
fn parse_declaration_variable_opaque() {
    let parsed: Declaration = syn::parse_quote! {
        opaque stuff<16>
    };
    let expected = Declaration::VariableOpaque {
        name: Ident("stuff".to_string()),
        max: Value::Const(Constant::Unsigned(16))
    };

    assert_eq!(parsed, expected);
}

#[test]
fn parse_spec() {
    let tokens = quote! {

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

        typedef int Number;

        union Blargh switch (int okay) {
            case 1:
                struct {
                    unsigned int red;
                    unsigned int green;
                    unsigned int blue;
                } color;
            case 0:
            case -1:
                opaque malady<>;
            default:
                void;
        };
    };

    let parsed: Specification = syn::parse2(tokens).unwrap_or_else(|e|
        panic!("Failed parsing Specification: {}", e));

    let expected_union = Definition::Type(TypeDef::Union(
            Ident("Blargh".to_string()),
            UnionBody {
                discriminant_decl: Declaration::Named {
                    name: Ident("okay".to_string()),
                    ty: TypeSpec::Int,
                },
                case_specs: vec![
                    CaseSpec {
                        cases: vec![
                            Value::Const(Constant::Unsigned(1)),
                        ],
                        decl: Declaration::Named {
                            name: Ident("color".to_string()),
                            ty: TypeSpec::Struct(StructBody(vec![
                                    Declaration::Named { ty: TypeSpec::Unsigned, name: Ident("red".to_string()) },
                                    Declaration::Named { ty: TypeSpec::Unsigned, name: Ident("green".to_string()) },
                                    Declaration::Named { ty: TypeSpec::Unsigned, name: Ident("blue".to_string()) },
                            ])),
                        },
                    },
                    CaseSpec {
                        cases: vec![
                            Value::Const(Constant::Unsigned(0)),
                            Value::Const(Constant::Signed(-1)),
                        ],
                        decl: Declaration::VariableOpaque {
                            name: Ident("malady".to_string()),
                            max: Value::Const(Constant::Unsigned(u32::MAX)),
                        },
                    },
                ],
                default_decl: Some(Declaration::Void),
            }));

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
            Definition::Type(TypeDef::Def(Declaration::Named {
                name: Ident("Number".to_string()),
                ty: TypeSpec::Int,
            })),
            expected_union,
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
