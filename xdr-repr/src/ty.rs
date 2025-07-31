#![allow(dead_code)]

use crate::xdr::*;

trait HasDefinition {
    fn def() -> Definition;
}

trait HasTypeDef {
    fn type_def() -> TypeDef;
}

impl<T: HasTypeDef> HasDefinition for T {
    fn def() -> Definition {
        Definition::Type(T::type_def())
    }
}

trait HasTypeSpec {
    fn type_spec() -> TypeSpec;
}

impl HasTypeSpec for i32 {
    fn type_spec() -> TypeSpec {
        TypeSpec::Int
    }
}

impl HasTypeSpec for u32 {
    fn type_spec() -> TypeSpec {
        TypeSpec::Unsigned
    }
}

trait Declare {
    fn declare(name: Ident) -> Declaration;
}

impl Declare for u32 {
    fn declare(name: Ident) -> Declaration {
        Declaration::Named {
            name,
            ty: TypeSpec::Unsigned,
        }
    }
}

impl<const N: usize> Declare for [u8; N] {
    fn declare(name: Ident) -> Declaration {
        Declaration::FixedOpaque {
            name,
            // TODO: use compile-time asserts to check N fits in u32
            size: Value::Const((N as u32).into()),
        }
    }
}

#[cfg(feature = "xdr_common")]
const _: () = {
    use xdr_common::BytesM;

    impl<const MAX: u32> Declare for BytesM<MAX> {
        fn declare(name: Ident) -> Declaration {
            Declaration::VariableOpaque {
                name,
                // TODO: use compile-time asserts to check MAX fits in u32
                max: Value::Const(Constant::Unsigned(MAX as u32))
            }
        }
    }
};

impl<T> Declare for T
    where T: HasTypeDef
{
    fn declare(name: Ident) -> Declaration {
        match T::type_def() {
            TypeDef::Def(decl) => decl,
            // Enum(Ident, EnumBody),
            TypeDef::Struct(ty_ident, _) =>
                Declaration::Named {
                    name,
                    ty: TypeSpec::Ident(ty_ident)
                },
            // Union(Ident, UnionBody)
        }
    }
}

/*
// conflicts with impl where T: HasTypeDef
impl<T> Declare for T
    where T: HasTypeSpec
{
    fn declare(name: Ident) -> Declaration {
        Declaration::Named {
            name,
            ty: T::type_spec(),
        }
    }
}
*/

#[cfg(test)]
mod tytest {
    use super::*;
    use crate::{check_format, spec};
    use crate::format::*;
    use crate::construct::*;

    struct Boo {
        boonum: u32,
    }

    impl HasTypeDef for Boo {
        fn type_def() -> TypeDef {
            struct_def("Boo", vec![
                <u32 as Declare>::declare(ident("boonum"))
            ])
        }
    }

    struct Foo {
        num: u32,
        bytes: [u8; 3],
        boo_by_name: Boo,
        // We can't actually inline this definition in Rust
        boo_inline: Boo,
    }

    // To be derived
    impl HasTypeDef for Foo {
        fn type_def() -> TypeDef {
            declare_type("Foo",
                struct_type(vec![
                    <u32>::declare(ident("num")),
                    <[u8; 3]>::declare(ident("bytes")),
                    <Boo>::declare(ident("boo_by_name")),

                    // We won't need to generate inline structs like this
                    declare_type("boo_inline", struct_type(vec![
                        declare_unsigned("boonum"),
                    ])),
                ])).into()
        }
    }

    struct Garbage {
        weight: u32,
    }

    // To be derived
    impl HasTypeDef for Garbage {
        fn type_def() -> TypeDef {
            struct_def("Garbage", vec![
                <u32>::declare(ident("weight")),
            ])
        }
    }

    check_format! { foo_format
        Foo::def()
        =>
            "typedef struct {"
            "	unsigned int num;"
            "	opaque bytes[3];"
            "	Boo boo_by_name;"
            "	struct {"
            "		unsigned int boonum;"
            "	} boo_inline;"
            "} Foo;"
    }

    const FOURTEEN: Constant = Constant::Signed(-14);

    check_format! { specification_format
        // spec(vec![Foo::def(), Garbage::def()])
        spec![Foo::def(), Garbage::def(), def_const("SPECIAL", FOURTEEN)]
        =>
            "typedef struct {"
            "	unsigned int num;"
            "	opaque bytes[3];"
            "	Boo boo_by_name;"
            "	struct {"
            "		unsigned int boonum;"
            "	} boo_inline;"
            "} Foo;"
            ""
            "struct Garbage {"
            "	unsigned int weight;"
            "};"
            ""
            "const SPECIAL = -14;"
            ""
    }

    #[cfg(feature = "xdr_common")]
    mod test_common {
        use super::*;
        use xdr_common::*;

        struct Varry {
            stuff: BytesM<100>,
        }

        // To be derived
        impl HasTypeDef for Varry {
            fn type_def() -> TypeDef {
                struct_def("Varry", vec![
                    <BytesM<100>>::declare(ident("stuff")),
                ])
            }
        }

        #[cfg(feature = "xdr_common")]
        check_format! { format_variable_opaque
            Varry::def()
            =>
            "struct Varry {"
            "	opaque stuff<100>;"
            "};"
        }
    }
}
