use xdr_repr::ty::*;
use xdr_repr::{check_format, assert_formatted, lines};
use xdr_repr::format::*;
use xdr_derive::HasTypeDef;

#[derive(HasTypeDef)]
pub struct Boo {
    pub boonum: u32,
}

#[derive(HasTypeDef)]
pub struct Foo {
    pub num: u32,
    pub bytes: [u8; 3],
    pub boo: Boo,
}

#[derive(HasTypeDef)]
pub struct Garbage {
    pub weight: u32,
}

check_format! { foo_format
    Foo::def()
    =>
        "struct Foo {"
        "	unsigned int num;"
        "	opaque bytes[3];"
        "	Boo boo;"
        "};"
}

#[cfg(feature = "xdr_common")]
pub mod test_common {
    use super::*;
    use xdr_common::*;

    #[derive(HasTypeDef)]
    pub struct Varry {
        pub stuff: BytesM<100>,
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
