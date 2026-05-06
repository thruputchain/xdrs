use xdr_repr::*;
use xdrs::*;

#[derive(XDR)]
pub struct Bare;

check_format! { derive_bare
    Bare::def()
    =>
        "struct Bare {"
        "	void;"
        "};"
}

#[derive(XDR)]
pub struct Boo {
    pub boonum: u32,
}

#[derive(XDR)]
pub struct Foo {
    pub num: u32,
    pub bytes: [u8; 3],
    pub boo: Boo,
}

#[derive(XDR)]
pub struct Garbage {
    pub weight: u32,
}

check_format! { derive_foo
    Foo::def()
    =>
        "struct Foo {"
        "	unsigned int num;"
        "	opaque bytes[3];"
        "	Boo boo;"
        "};"
}

pub mod unnamed {
    use super::*;

    #[derive(XDR)]
    pub struct Unnamed(pub u32, pub i32);

    check_format! { derive
        Unnamed::def()
        =>
            "struct Unnamed {"
            "	unsigned int f0;"
            "	int f1;"
            "};"
    }
}

pub mod test_common {
    use super::*;

    #[derive(XDR)]
    pub struct Varry {
        pub stuff: BytesM<100>,
    }

    check_format! { derive_variable_opaque
        Varry::def()
        =>
        "struct Varry {"
        "	opaque stuff<100>;"
        "};"
    }
}

pub mod custom_name {
    use super::*;

    #[derive(XDR)]
    #[xdr(name = "PersonRecord")]
    pub struct Person {
        pub name: Vec<u8>,
        pub age: u32,
    }

    check_format! { derive_custom_name_struct
        Person::def()
        =>
        "struct PersonRecord {"
        "	opaque name<>;"
        "	unsigned int age;"
        "};"
    }

    #[derive(XDR)]
    #[xdr(name = "CustomStructName")]
    pub struct CustomNamed {
        pub field1: i32,
        pub field2: bool,
    }

    check_format! { derive_custom_named
        CustomNamed::def()
        =>
        "struct CustomStructName {"
        "	int field1;"
        "	bool field2;"
        "};"
    }

    #[derive(XDR)]
    #[xdr(name = "EmptyStruct")]
    pub struct Empty;

    check_format! { derive_custom_name_empty
        Empty::def()
        =>
        "struct EmptyStruct {"
        "	void;"
        "};"
    }
}

pub mod typedef_backed_fields {
    use super::*;

    pub struct AccountId;

    impl HasTypeDef for AccountId {
        fn type_def() -> TypeDef {
            TypeDef::Def(declare_unsigned("AccountId"))
        }
    }

    #[derive(XDR)]
    pub struct Payment {
        pub payer: AccountId,
        pub amount: u32,
    }

    check_format! { typedef_field_preserves_parent_name
        Payment::def()
        =>
            "struct Payment {"
            "	AccountId payer;"
            "	unsigned int amount;"
            "};"
    }

    pub struct WrappedBool;

    impl HasTypeDef for WrappedBool {
        fn type_def() -> TypeDef {
            TypeDef::Def(declare_type("WrappedBool", TypeSpec::Bool))
        }
    }

    #[derive(XDR)]
    pub struct Flags {
        pub active: WrappedBool,
        pub count: i32,
    }

    check_format! { typedef_bool_field_preserves_parent_name
        Flags::def()
        =>
            "struct Flags {"
            "	WrappedBool active;"
            "	int count;"
            "};"
    }
}

pub mod mixed_void_fields {
    use super::*;

    #[derive(XDR, ReadXDR, WriteXDR, Debug, PartialEq, Eq)]
    pub struct MixedVoid {
        pub x: u32,
        pub gap1: (),
        pub y: i32,
        pub gap2: (),
    }

    check_format! { mixed_void_struct_renders_correctly
        MixedVoid::def()
        =>
            "struct MixedVoid {"
            "	unsigned int x;"
            "	void;"
            "	int y;"
            "	void;"
            "};"
    }
}

pub mod tuple_fields {
    use super::*;

    // Test struct with a simple tuple field
    #[derive(XDR)]
    pub struct Foo {
        pub x: (u32, u64),
    }

    check_format! { derive_struct_with_tuple
        Foo::def()
        =>
        "struct Foo {"
        "	struct {"
        "		unsigned int unnamed_0;"
        "		unsigned hyper unnamed_1;"
        "	} x;"
        "};"
    }

    // Test struct with multiple fields including tuple
    #[derive(XDR)]
    pub struct Bar {
        pub id: u32,
        pub coords: (i32, i32),
        pub active: bool,
    }

    check_format! { derive_struct_with_mixed_fields
        Bar::def()
        =>
        "struct Bar {"
        "	unsigned int id;"
        "	struct {"
        "		int unnamed_0;"
        "		int unnamed_1;"
        "	} coords;"
        "	bool active;"
        "};"
    }

    // Test struct with nested tuple
    #[derive(XDR)]
    pub struct Nested {
        pub data: (u32, (i32, bool)),
    }

    check_format! { derive_struct_with_nested_tuple
        Nested::def()
        =>
        "struct Nested {"
        "	struct {"
        "		unsigned int unnamed_0;"
        "		struct {"
        "			int unnamed_0;"
        "			bool unnamed_1;"
        "		} unnamed_1;"
        "	} data;"
        "};"
    }
}

pub mod string_fields {
    use super::*;

    #[derive(XDR)]
    pub struct Named {
        pub label: String,
    }

    check_format! { derive_string_field
        Named::def()
        =>
            "struct Named {"
            "	string label<>;"
            "};"
    }
}

pub mod boxed_fields {
    use super::*;

    #[derive(XDR)]
    pub struct Inner {
        pub value: u32,
    }

    #[derive(XDR)]
    pub struct BoxedNamed {
        pub inner: Box<Inner>,
    }

    check_format! { boxed_named_struct
        BoxedNamed::def()
        =>
            "struct BoxedNamed {"
            "	Inner inner;"
            "};"
    }

    #[derive(XDR)]
    pub struct BoxedBlob {
        pub blob: Box<BytesM<4>>,
    }

    check_format! { boxed_bytes
        BoxedBlob::def()
        =>
            "struct BoxedBlob {"
            "	opaque blob<4>;"
            "};"
    }

    #[derive(XDR)]
    pub struct BoxedText {
        pub text: Box<StringM<8>>,
    }

    check_format! { boxed_string
        BoxedText::def()
        =>
            "struct BoxedText {"
            "	string text<8>;"
            "};"
    }

    #[derive(XDR)]
    pub struct BoxedValues {
        pub values: Box<VecM<u32, 3>>,
    }

    check_format! { boxed_vecm
        BoxedValues::def()
        =>
            "struct BoxedValues {"
            "	unsigned int values<3>;"
            "};"
    }

    #[derive(XDR)]
    pub struct BoxedOpaqueVec {
        pub data: Box<VecM<u8, 5>>,
    }

    check_format! { boxed_vecm_u8
        BoxedOpaqueVec::def()
        =>
            "struct BoxedOpaqueVec {"
            "	opaque data<5>;"
            "};"
    }

    #[derive(XDR)]
    pub struct BoxedOptional {
        pub maybe: Option<Box<Inner>>,
    }

    check_format! { boxed_optional
        BoxedOptional::def()
        =>
            "struct BoxedOptional {"
            "	Inner *maybe;"
            "};"
    }
}
