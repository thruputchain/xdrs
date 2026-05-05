#![allow(dead_code)]

use xdrs::xdr_repr::*;
use xdrs::*;

#[derive(XDR)]
struct Thing {
    okay: bool,
}

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

pub mod opaque_variable {
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

pub mod arrays_variable {
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
    struct Thing {
        okay: bool,
    }

    #[derive(XDR)]
    struct ContainsMultitudes<'a> {
        n_valid: u32,
        numbers: VecM<u32, 100>,
        some_things: [Thing; 3],
        more_things: &'a [Thing],
        many_things: VecM<Thing, 3>,
    }

    check_format! { derive
        ContainsMultitudes::def()
        =>
            "struct ContainsMultitudes {"
            "	unsigned int n_valid;"
            "	unsigned int numbers<100>;"
            "	Thing some_things[3];"
            "	Thing more_things<>;"
            "	Thing many_things<3>;"
            "};"
    }
}

pub mod vec_variable {
    use super::*;

    #[derive(ReadXDR, WriteXDR, XDR)]
    struct Batch {
        items: Vec<u32>,
    }

    check_format! { derive
        Batch::def()
        =>
            "struct Batch {"
            "	unsigned int items<>;"
            "};"
    }

    #[derive(ReadXDR, WriteXDR, XDR)]
    struct Element {
        okay: bool,
    }

    #[derive(ReadXDR, WriteXDR, XDR)]
    struct ElementList {
        entries: Vec<Element>,
    }

    check_format! { derive_named_type
        ElementList::def()
        =>
            "struct ElementList {"
            "	Element entries<>;"
            "};"
    }
}

pub mod variable_array_of_variable_array {
    use super::*;

    // We can't use `type Inner = VecM<u32, 100>` here, as that just gives us an alias
    // to an XDR variable-array.  Arrays can't be expressed with a type-specifier, which
    // is needed to represent the element type of the outer array.  So this inner array
    // type must be given a name by defining a struct containing it; the name can be
    // used to form the necessary type-specifier.
    #[derive(XDR)]
    struct Inner(VecM<u32, 100>);

    type Outer = VecM<Inner, 2>;

    fn define_outer() -> Definition {
        let name = Ident("Outer".to_owned());
        let typedef: TypeDef = Outer::declare(name).into();
        Definition::Type(typedef)
    }

    check_format! { format_typedef_outer
        define_outer()
        =>
            "typedef Inner Outer<2>;"
    }
}
