// use xdr_repr::xdr::*;
use xdr_repr::ty::*;
use xdr_repr::{check_format, assert_formatted, lines};
use xdr_repr::format::*;
use xdr_repr::construct::*;
use xdr_derive::XDR;

use pretty_assertions::assert_eq;

// Unit-only enum

#[derive(XDR)]
#[repr(i32)]
pub enum UnitOnly {
    Alpha = 1,
    Beta = 2,
    Omega = -1,
}

check_format! { derive_enum_unitonly
    UnitOnly::def()
    =>
        "enum UnitOnly {"
        "	Alpha = 1;"
        "	Beta = 2;"
        "	Omega = -1;"
        "};"
}

// Field-less enum

#[derive(XDR)]
#[repr(i32)]
pub enum Fieldless {
    Tuple(),
    Struct{},
    Unit,
}

check_format! { derive_enum_fieldless
    Fieldless::def()
    =>
        "enum Fieldless {"
        "	Tuple = 0;"
        "	Struct = 1;"
        "	Unit = 2;"
        "};"
}

#[derive(XDR)]
#[repr(i32)]
pub enum Status {
    Ok = 0,
    Err = -1,
}

check_format! { derive_enum_status
    Status::def()
    =>
        "enum Status {"
        "	Ok = 0;"
        "	Err = -1;"
        "};"
}

#[derive(XDR)]
#[repr(i32)]
pub enum Message {
    Good {
        contents: [u8; 100],
    } = Status::Ok as i32,
    Bad {
        err_msg: [u8; 20],
    } = Status::Err as i32,
}

// TODO: use attrs to avoid production of the additional enum
check_format! { derive_message
    Message::spec()
    =>
        "enum MessageType {"
        "	Good = 0;"
        "	Bad = -1;"
        "};"
        ""
        "union Message switch (MessageType which)"
        "{"
        "case Good:"
        "	struct {"
        "		opaque contents[100];"
        "	} good;"
        "case Bad:"
        "	struct {"
        "		opaque err_msg[20];"
        "	} bad;"
        "};"
        ""
}

/*
// Enum with the wrong discriminant representation for XDR
// (Shouldn't compile.)
#[derive(XDR)]
#[repr(u8)]
pub enum EnumBadRepr {
    Alpha = 1,
}

check_format! { #[should_panic] derive_enum_bad_repr
    EnumBadRepr::def()
    =>
        "enum EnumBadRepr {"
        "	Alpha = 1;"
        "};"
}
*/

// Data-bearing enum

#[derive(XDR)]
#[repr(i32)]
pub enum Data {
    Alpha {
        n: u32,
        okay: bool,
    } = 1,
    Beta = 2,
    Omega = -1,
}

check_format! { derive_enum_data
    Data::spec()
    =>
        "enum DataType {"
        "	Alpha = 1;"
        "	Beta = 2;"
        "	Omega = -1;"
        "};"
        ""
        "union Data switch (DataType which)"
        "{"
        "case Alpha:"
        "	struct {"
        "		unsigned int n;"
        "		bool okay;"
        "	} alpha;"
        "case Beta:"
        "	void;"
        "case Omega:"
        "	void;"
        "};"
        ""
}

// Enum with a default arm
// TODO: fix default handling: same as other case-specs

#[derive(XDR)]
#[repr(i32)]
pub enum EnumWithDefault {
    Alpha = 10,
    Beta = 20,
    // #[xdrs(enum_default)]
    Default { okay: bool },
}

check_format! { derive_enum_with_default
    EnumWithDefault::spec()
    =>
        "enum EnumWithDefaultType {"
        "	Alpha = 10;"
        "	Beta = 20;"
        "};"
        ""
        "union EnumWithDefault switch (EnumWithDefaultType which) {"
        "	case Alpha:"
        "		void;"
        "	case Beta:"
        "		void;"
        "	default:"
        "		struct {"
        "			okay: bool;"
        "		};"
        "};"
        ""
}

// Unit-only enum with all-implicit discriminants

#[derive(XDR)]
#[repr(i32)]
pub enum ImplicitUnit {
    Alpha,
    Beta,
    Omega,
}

check_format! { derive_enum_implicit_unit
    ImplicitUnit::spec()
    =>
        "enum ImplicitUnit {"
        "	Alpha = 0;"
        "	Beta = 1;"
        "	Omega = 2;"
        "};"
        ""
}

// Enum with all-implicit discriminants

#[derive(XDR)]
#[repr(i32)]
pub enum Implicit {
    Alpha {
        n: u32,
        okay: bool,
    },
    Beta,
    Omega,
}

check_format! { derive_enum_implicit
    Implicit::spec()
    =>
        "enum ImplicitType {"
        "	Alpha = 0;"
        "	Beta = 1;"
        "	Omega = 2;"
        "};"
        ""
        "union Implicit switch (ImplicitType which)"
        "{"
        "case Alpha:"
        "	struct {"
        "		unsigned int n;"
        "		bool okay;"
        "	} alpha;"
        "case Beta:"
        "	void;"
        "case Omega:"
        "	void;"
        "};"
        ""
}

/*
#[derive(XDR)]
#[repr(i32)]
pub enum FunctionLike {
    Beta(i32),
    Omega,
}
*/


/*
// Enum with mixed implicit/explicit discriminants (and no default arm)
// (currrently disallowed)

#[derive(XDR)]
#[repr(i32)]
pub enum Mixed {
    Beta, // = 0
    Omega = 3,
}

check_format! { #[should_panic] derive_enum_mixed
    Mixed::spec()
    =>
        "enum Mixed {"
        "	Beta = 0;"
        "	Omega = 3;"
        "};"
}
*/
