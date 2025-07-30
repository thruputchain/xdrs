#![allow(dead_code)]

use crate::xdr::*;

trait HasDefinition {
    fn def() -> Definition;
}

trait HasTypeDef {
    fn type_def() -> TypeDef;
}


#[test]
fn test() {
    use construct::*;
    use TypeDef;

    struct Foo {
        num: u32,
        bytes: [u8; 3],
    }

    impl HasTypeDef for Foo {
        fn type_def() -> TypeDef {
            /*
            TypeDef::Struct(
                ident("Foo"),
                StructBody(vec![
                    declare_unsigned("num"),
                    declare_fixed_opaque("bytes", 3),
                ]),
            )
            */

            TypeDef::Def(declare_type("foo",
                TypeSpec::Struct(
                    StructBody(vec![
                        declare_unsigned("num"),
                        declare_fixed_opaque("bytes", 3),
                    ]))))
        }
    }

    let expected =
           "typedef struct {\
                unsigned int num;\
                opaque bytes[3];\
            } foo;\
            ";

    let output = Foo::type_def().to_string();
    println!("OUT:\n{output}");
    assert_eq!(output, expected);
}

pub mod construct {
    use crate::xdr::*;

    pub fn ident(s: &str) -> Ident {
        Ident(s.to_string())
    }

    pub fn constant(n: u32) -> Value {
        Value::Const(Constant(n))
    }

    pub fn name_ref(s: &str) -> Value {
        Value::Ident(ident(s))
    }

    pub fn declare_type(s: &str, type_spec: TypeSpec) -> Declaration {
        Declaration::Named {
            name: ident(s),
            ty: type_spec,
        }
    }

    pub fn declare_fixed_opaque(s: &str, size: u32) -> Declaration {
        Declaration::FixedOpaque {
            name: ident(s),
            size: constant(size),
        }
    }

    pub fn declare_unsigned(s: &str) -> Declaration {
        declare_type(s, TypeSpec::UnsignedInt)
    }

    pub fn declare_int(s: &str) -> Declaration {
        declare_type(s, TypeSpec::Int)
    }

    pub fn declare_named_type(s: &str, ident: Ident) -> Declaration {
        declare_type(s, TypeSpec::Ident(ident))
    }
}
