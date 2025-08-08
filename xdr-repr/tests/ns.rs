use xdr_repr::xdr::*;
use xdr_repr::ty::*;
use xdr_repr::{check_format, assert_formatted, lines};
use xdr_repr::format::*;
use xdr_repr::construct::*;

pub struct Garbage {
    pub weight: u32,
}

impl HasTypeDef for Garbage {
    fn type_def() -> TypeDef {
        struct_def("Garbage", vec![
            <u32>::declare(ident("weight")),
        ])
    }
}

check_format! { garbage_spec
    namespace("transfer_station", specification(vec![Garbage::def()]))
    =>
        "namespace transfer_station"
        "{"
        ""
        "struct Garbage {"
        "	unsigned int weight;"
        "};"
        ""
        "}"
}
