#![allow(dead_code)]

use xdr_repr::{check_format, assert_formatted, lines};
use xdr_repr::*;

#[derive(XDR)]
struct Thing { okay: bool }

pub mod opaque_fixed {
    use super::*;

    #[derive(XDR)]
    struct Bytes {
        bytes: [u8; 100],
    }

    check_format! { derive
        Bytes::def()
        =>
            "struct Bytes {"
            "	opaque bytes[100];"
            "};"
    }
}

pub mod arrays_fixed {
    use super::*;

    #[derive(XDR)]
    struct ContainsMultitudes {
        n_valid: u32,
        numbers: [u32; 100],
        some_things: [Thing; 3],
    }

    check_format! { derive
        ContainsMultitudes::def()
        =>
            "struct ContainsMultitudes {"
            "	unsigned int n_valid;"
            "	unsigned int numbers[100];"
            "	Thing some_things[3];"
            "};"
    }
}

pub mod optional {
    use super::*;

    #[derive(XDR)]
    struct Bytes {
        bytes: [u8; 100],
    }

    #[derive(XDR)]
    struct OptStruct {
        maybe_bytes: Option<Bytes>,
    }

    check_format! { derive
        OptStruct::def()
        =>
            "struct OptStruct {"
            "	Bytes *maybe_bytes;"
            "};"
    }
}


#[cfg(feature = "xdr_common")]
pub mod opaque_variable {
    use xdr_common::*;
    use super::*;

    #[derive(XDR)]
    struct Bytes {
        bytes: BytesM<1000>,
    }

    check_format! { derive
        Bytes::def()
        =>
            "struct Bytes {"
            "	opaque bytes<1000>;"
            "};"
    }
}

#[cfg(feature = "xdr_common")]
pub mod arrays_variable {
    use xdr_common::*;
    use super::*;

    type T = VecM<u32, 100>;

    fn make() -> Definition {
        let name = Ident("IntVec".to_owned());
        let typedef: TypeDef = T::declare(name).into();
        Definition::Type(typedef)
    }

    check_format! { format_typedef
        make()
        =>
            "typedef unsigned int IntVec<100>;"
    }

    #[derive(XDR)]
    struct Thing { okay: bool }

    #[derive(XDR)]
    struct ContainsMultitudes {
        n_valid: u32,
        numbers: VecM<u32, 100>,
        some_things: [Thing; 3],
        many_things: VecM<Thing, 3>,
    }

    check_format! { derive
        ContainsMultitudes::def()
        =>
            "struct ContainsMultitudes {"
            "	unsigned int n_valid;"
            "	unsigned int numbers<100>;"
            "	Thing some_things[3];"
            "	Thing many_things<3>;"
            "};"
    }
}
