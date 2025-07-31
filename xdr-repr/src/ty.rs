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

#[cfg(test)]
mod tytest {
    use super::*;
    use crate::{check_format, spec};
    use crate::format::*;
    use crate::construct::*;

    struct Boo {
        boonum: u32,
    }

    struct Foo {
        num: u32,
        bytes: [u8; 3],
        boo: Boo,
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
