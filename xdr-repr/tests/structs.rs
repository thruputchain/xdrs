use xdr_repr::*;

pub struct Boo {
    pub boonum: u32,
}

impl HasTypeDef for Boo {
    fn type_def() -> TypeDef {
        struct_def("Boo", vec![<u32 as Declare>::declare(ident("boonum"))])
    }
}

pub struct Foo {
    pub num: u32,
    pub bytes: [u8; 3],
    pub boo_by_name: Boo,
    // We can't actually inline this definition in Rust
    pub boo_inline: Boo,
}

impl HasTypeDef for Foo {
    fn type_def() -> TypeDef {
        declare_type(
            "Foo",
            struct_type(vec![
                <u32>::declare(ident("num")),
                <[u8; 3]>::declare(ident("bytes")),
                <Boo>::declare(ident("boo_by_name")),
                // We won't need to generate inline structs like this
                declare_type("boo_inline", struct_type(vec![declare_unsigned("boonum")])),
            ]),
        )
        .into()
    }
}

pub struct Garbage {
    pub weight: u32,
}

impl HasTypeDef for Garbage {
    fn type_def() -> TypeDef {
        struct_def("Garbage", vec![<u32>::declare(ident("weight"))])
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

#[test]
fn type_ref() {
    assert_eq!(Foo::type_def(), <&Foo>::type_def());
}

const FOURTEEN: Constant = Constant::Signed(-14);

check_format! { specification_format
    specification(vec![Foo::def(), Garbage::def(), def_const("SPECIAL", FOURTEEN)])
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

pub struct IntTypes {
    pub signed_short: i32,
    pub unsigned_short: u32,
    pub signed_long: i64,
    pub unsigned_long: u64,
}

impl HasTypeDef for IntTypes {
    fn type_def() -> TypeDef {
        struct_def(
            "IntTypes",
            vec![
                <i32>::declare(ident("signed_short")),
                <u32>::declare(ident("unsigned_short")),
                <i64>::declare(ident("signed_long")),
                <u64>::declare(ident("unsigned_long")),
            ],
        )
    }
}

check_format! { int_types_format
    IntTypes::def()
    =>
        "struct IntTypes {"
        "	int signed_short;"
        "	unsigned int unsigned_short;"
        "	hyper signed_long;"
        "	unsigned hyper unsigned_long;"
        "};"
}

#[cfg(feature = "xdr_common")]
pub mod test_common {
    use super::*;
    use xdr_common::*;

    pub struct Varry<'a> {
        pub stuff: BytesM<100>,
        pub slice: &'a [u8],
    }

    impl<'a> HasTypeDef for Varry<'a> {
        fn type_def() -> TypeDef {
            struct_def(
                "Varry",
                vec![
                    <BytesM<100>>::declare(ident("stuff")),
                    <&[u8]>::declare(ident("slice")),
                ],
            )
        }
    }

    check_format! { format_variable_opaque
        Varry::def()
        =>
        "struct Varry {"
        "	opaque stuff<100>;"
        "	opaque slice<>;"
        "};"
    }

    pub struct VecTest {
        pub foo_vec: Vec<u8>,
    }

    impl HasTypeDef for VecTest {
        fn type_def() -> TypeDef {
            struct_def("VecTest", vec![<Vec<u8>>::declare(ident("foo_vec"))])
        }
    }

    check_format! { format_variable_array
        VecTest::def()
        =>
        "struct VecTest {"
        "	opaque foo_vec<>;"
        "};"
    }

    pub struct StringTest {
        pub name: StringM<10>,
    }

    impl HasTypeDef for StringTest {
        fn type_def() -> TypeDef {
            struct_def("StringTest", vec![<StringM<10>>::declare(ident("name"))])
        }
    }

    check_format! { format_string
        StringTest::def()
        =>
        "struct StringTest {"
        "	string name<10>;"
        "};"
    }
}
