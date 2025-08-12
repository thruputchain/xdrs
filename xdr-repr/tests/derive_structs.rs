use xdr_derive::XDR;
use xdr_repr::*;
use xdr_repr::{check_format, assert_formatted, lines};

// // compile error: XDR: can't represent struct with no fields
// #[derive(XDR)]
// pub struct Bare;

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
    pub struct Unnamed(
        pub u32,
        pub i32,
    );

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
    use xdr_common::*;

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
