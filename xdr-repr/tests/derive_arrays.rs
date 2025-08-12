#![allow(dead_code)]

use xdr_repr::{check_format, assert_formatted, lines};
use xdr_repr::*;
use xdr_derive::XDR;

#[derive(XDR)]
struct Thing { okay: bool }

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
