use xdr_repr::xdr::*;
use xdr_repr::ty::*;
use xdr_repr::{check_format, assert_formatted, lines};
use xdr_repr::format::*;
use xdr_repr::construct::*;

#[repr(i32)]
pub enum E {
    Alpha = 1,
    Beta = 2,
    Omega = (-1),
}

impl HasTypeDef for E {
    fn type_def() -> TypeDef {
        enum_def("E", vec![
            (ident("Alpha"), signed(1)),
            (ident("Beta"), signed(2)),
            (ident("Omega"), signed(-1)),
        ])
    }
}

check_format! { enum_format
    E::def()
    =>
        "enum E {"
        "	Alpha = 1;"
        "	Beta = 2;"
        "	Omega = -1;"
        "};"
}

pub struct S {
    pub my_enum: E,
    pub my_enum_inline: E, // Can't actually do inline enum in Rust
}

impl HasTypeDef for S {
    fn type_def() -> TypeDef {
        struct_def("S", vec![
            <E>::declare(ident("my_enum")),

            declare_type("my_enum_inline", enum_type(vec![
                (ident("Alpha"), signed(1)),
                (ident("Beta"), signed(2)),
                (ident("Omega"), signed(-1)),
            ]))
        ])
    }
}

check_format! { inline_enum_format
    S::def()
    =>
        "struct S {"
        "	E my_enum;"
        "	enum {"
        "		Alpha = 1;"
        "		Beta = 2;"
        "		Omega = -1;"
        "	} my_enum_inline;"
        "};"
}
