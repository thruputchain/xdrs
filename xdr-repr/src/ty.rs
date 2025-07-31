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

// conflicts with impl where T: HasTypeDef
#[cfg(any())]
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
        boo: Boo,
        boo_by_name: Boo,
    }

    impl HasTypeDef for Foo {
        fn type_def() -> TypeDef {
            declare_type("Foo",
                struct_type(vec![
                    declare_unsigned("num"),
                    declare_fixed_opaque("bytes", 3),
                    declare_type("boo", struct_type(vec![
                        declare_unsigned("boonum"),
                    ])),
                    Boo::declare(ident("boo_by_name")),
                ])).into()
        }
    }

    struct Garbage {
        weight: u32,
    }

    impl HasTypeDef for Garbage {
        fn type_def() -> TypeDef {
            struct_def("Garbage", vec![
                declare_unsigned("weight"),
            ])
        }
    }

    check_format! { foo_format
        Foo::def()
        =>
            "typedef struct {"
            "	unsigned int num;"
            "	opaque bytes[3];"
            "	struct {"
            "		unsigned int boonum;"
            "	} boo;"
            "	Boo boo_by_name;"
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
            "	struct {"
            "		unsigned int boonum;"
            "	} boo;"
            "	Boo boo_by_name;"
            "} Foo;"
            ""
            "struct Garbage {"
            "	unsigned int weight;"
            "};"
            ""
            "const SPECIAL = -14;"
            ""
    }
}
