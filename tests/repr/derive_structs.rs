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

#[cfg(feature = "xdr_common")]
pub mod test_common {
    use super::*;

    #[derive(XDR)]
    pub struct Varry {
        pub stuff: BytesM<100>,
    }

    #[cfg(feature = "xdr_common")]
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
