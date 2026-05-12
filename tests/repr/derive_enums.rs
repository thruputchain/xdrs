use xdr_repr::*;
use xdrs::*;

pub mod unit_only_enum {
    use super::*;

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
            "	Alpha = 1,"
            "	Beta = 2,"
            "	Omega = -1,"
            "};"
    }
}

pub mod fieldless_enum {
    use super::*;

    #[derive(XDR)]
    #[repr(i32)]
    pub enum Fieldless {
        Tuple(),
        Struct {},
        Unit,
    }

    check_format! { derive_enum_fieldless
        Fieldless::def()
        =>
            "enum Fieldless {"
            "	Tuple = 0,"
            "	Struct = 1,"
            "	Unit = 2,"
            "};"
    }
}

// Discriminant named by separate enum
pub mod separate_discriminant {
    use super::*;

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
            "	Ok = 0,"
            "	Err = -1,"
            "};"
    }

    #[derive(XDR)]
    #[xdr(discrim_type = "Status", discrim_var = "stat")]
    #[repr(i32)]
    pub enum Message {
        Ok { contents: [u8; 100] } = Status::Ok as i32,
        Err { err_msg: [u8; 20], code: i32 } = Status::Err as i32,
    }

    check_format! { check
        Message::spec()
        =>
            "union Message switch (Status stat)"
            "{"
            "case Ok:"
            "	opaque contents[100];"
            "case Err:"
            "	struct {"
            "		opaque err_msg[20];"
            "		int code;"
            "	} err;"
            "};"
            ""
    }
}

// Enum with the wrong discriminant representation for XDR
// (Shouldn't compile.)
#[cfg(any())]
pub mod wrong_discriminant {
    #[derive(XDR)]
    #[repr(u8)]
    pub enum EnumBadRepr {
        Alpha = 1,
    }

    #[cfg(any())]
    check_format! { #[should_panic] derive_enum_bad_repr
        EnumBadRepr::def()
        =>
            "enum EnumBadRepr {"
            "	Alpha = 1,"
            "};"
    }
}

pub mod enum_with_data {
    use super::*;

    #[derive(XDR)]
    #[repr(i32)]
    pub enum Data {
        Alpha { n: u32, okay: bool } = 1,
        Beta { b: i32 } = 2,
        Omega = -1,
    }

    check_format! { derive_enum
        Data::def()
        =>
            "union Data switch (DataType which)"
            "{"
            "case Alpha:"
            "	struct {"
            "		unsigned int n;"
            "		bool okay;"
            "	} alpha;"
            "case Beta:"
            "	int b;"
            "case Omega:"
            "	void;"
            "};"
    }

    check_format! { derive_spec
        Data::spec()
        =>
            "enum DataType {"
            "	Alpha = 1,"
            "	Beta = 2,"
            "	Omega = -1,"
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
            "	int b;"
            "case Omega:"
            "	void;"
            "};"
            ""
    }
}

// Fieldless enum with a default arm
// (this shouldn't compile)
#[cfg(any())]
pub mod fieldless_with_default {
    use super::*;

    #[derive(XDR)]
    #[repr(i32)]
    pub enum FieldlessDefault {
        Alpha = 10,
        Beta = 20,

        #[xdr(default_variant)]
        Other,
    }
}

pub mod enum_with_default {
    use super::*;

    #[derive(XDR)]
    #[repr(i32)]
    pub enum EnumWithDefault {
        Alpha {
            a: u32,
        } = 10,
        Beta = 20,

        #[xdr(default_variant)]
        Other {
            which: i32,
        },
    }

    check_format! { derive
        EnumWithDefault::spec()
        =>
            "enum EnumWithDefaultType {"
            "	Alpha = 10,"
            "	Beta = 20,"
            "};"
            ""
            "union EnumWithDefault switch (EnumWithDefaultType which)"
            "{"
            "case Alpha:"
            "	unsigned int a;"
            "case Beta:"
            "	void;"
            "default:"
            "	void;"
            "};"
            ""
    }
}

pub mod default_only {
    use super::*;

    #[derive(XDR)]
    #[repr(i32)]
    pub enum CatchAll {
        #[xdr(default_variant)]
        Default(i32),
    }

    check_format! { derive_default_only
        CatchAll::spec()
        =>
            "enum CatchAllType {"
            "};"
            ""
            "union CatchAll switch (CatchAllType which)"
            "{"
            "default:"
            "	void;"
            "};"
            ""
    }
}

pub mod default_with_payload {
    use super::*;

    #[derive(XDR)]
    #[repr(i32)]
    pub enum DefaultPayload {
        Alpha { a: u32 } = 10,
        Beta = 20,

        #[xdr(default_variant)]
        Default { code: i32, data: u32 },
    }

    check_format! { derive_default_payload
        DefaultPayload::spec()
        =>
            "enum DefaultPayloadType {"
            "	Alpha = 10,"
            "	Beta = 20,"
            "};"
            ""
            "union DefaultPayload switch (DefaultPayloadType which)"
            "{"
            "case Alpha:"
            "	unsigned int a;"
            "case Beta:"
            "	void;"
            "default:"
            "	unsigned int data;"
            "};"
            ""
    }
}

// Unit-only enum with all-implicit discriminants
pub mod implicit_unit {
    use super::*;

    #[derive(XDR)]
    #[repr(i32)]
    pub enum ImplicitUnit {
        Alpha,
        Beta,
        Omega,
    }

    check_format! { derive_enum_implicit_unit
        ImplicitUnit::def()
        =>
            "enum ImplicitUnit {"
            "	Alpha = 0,"
            "	Beta = 1,"
            "	Omega = 2,"
            "};"
    }
}

// Enum with all-implicit discriminants
pub mod enum_implicit {
    use super::*;

    #[derive(XDR)]
    #[repr(i32)]
    pub enum Implicit {
        Alpha { n: u32, okay: bool },
        Beta,
        Omega,
    }

    check_format! { derive_enum_implicit
        Implicit::spec()
        =>
            "enum ImplicitType {"
            "	Alpha = 0,"
            "	Beta = 1,"
            "	Omega = 2,"
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
}

pub mod unnamed_fields {
    use super::*;

    #[derive(XDR)]
    #[repr(i32)]
    pub enum Unnamed {
        Beta(i32),
        Omega,
    }

    check_format! { unnamed_fields
        Unnamed::spec()
        =>
            "enum UnnamedType {"
            "	Beta = 0,"
            "	Omega = 1,"
            "};"
            ""
            "union Unnamed switch (UnnamedType which)"
            "{"
            "case Beta:"
            "	int beta;"
            "case Omega:"
            "	void;"
            "};"
            ""
    }
}

// Enum with mixed implicit/explicit discriminants (and no default arm)
// TODO:
#[cfg(any())]
pub mod mixed {
    use super::*;

    #[derive(XDR)]
    #[repr(i32)]
    pub enum Mixed {
        Beta, // = 0
        Omega = 3,
    }

    #[cfg(any())]
    check_format! { derive_enum_mixed
        Mixed::spec()
        =>
            "enum Mixed {"
            "	Beta = 0,"
            "	Omega = 3,"
            "};"
    }
}

// Enum with empty struct/tuple variants
pub mod empty_variants {
    use super::*;

    #[derive(XDR)]
    #[repr(i32)]
    pub enum EmptyVariants {
        Alpha { a: u32 } = 1,
        Beta {} = 2,
        Gamma() = 3,
        Delta = 4,
    }

    check_format! { derive_empty_variants
        EmptyVariants::spec()
        =>
            "enum EmptyVariantsType {"
            "	Alpha = 1,"
            "	Beta = 2,"
            "	Gamma = 3,"
            "	Delta = 4,"
            "};"
            ""
            "union EmptyVariants switch (EmptyVariantsType which)"
            "{"
            "case Alpha:"
            "	unsigned int a;"
            "case Beta:"
            "	void;"
            "case Gamma:"
            "	void;"
            "case Delta:"
            "	void;"
            "};"
            ""
    }
}

pub mod multi_field_tuple {
    use super::*;

    #[derive(XDR)]
    #[repr(i32)]
    pub enum MultiTuple {
        Data(u32, u64) = 0,
        Empty = 1,
    }

    check_format! { derive_multi_field_tuple
        MultiTuple::spec()
        =>
            "enum MultiTupleType {"
            "	Data = 0,"
            "	Empty = 1,"
            "};"
            ""
            "union MultiTuple switch (MultiTupleType which)"
            "{"
            "case Data:"
            "	struct {"
            "		unsigned int f0;"
            "		unsigned hyper f1;"
            "	} data;"
            "case Empty:"
            "	void;"
            "};"
            ""
    }
}

pub mod custom_name {
    use super::*;

    #[derive(XDR)]
    #[xdr(name = "StatusEnum")]
    #[repr(i32)]
    pub enum Status {
        Ok = 0,
        Error = 1,
    }

    check_format! { derive_custom_name_enum
        Status::def()
        =>
        "enum StatusEnum {"
        "	Ok = 0,"
        "	Error = 1,"
        "};"
    }

    #[derive(XDR)]
    #[xdr(name = "ResponseUnion")]
    #[repr(i32)]
    pub enum Response {
        Success { data: [u8; 100] } = 0,
        NotFound = 1,
    }

    check_format! { derive_custom_name_union
        Response::spec()
        =>
        "enum ResponseType {"
        "	Success = 0,"
        "	NotFound = 1,"
        "};"
        ""
        "union ResponseUnion switch (ResponseType which)"
        "{"
        "case Success:"
        "	opaque data[100];"
        "case NotFound:"
        "	void;"
        "};"
        ""
    }
}

pub mod default_tuple_payload {
    use super::*;

    #[derive(XDR)]
    #[repr(i32)]
    pub enum DefaultTuplePayload {
        Alpha { a: u32 } = 10,
        Beta = 20,

        #[xdr(default_variant)]
        Default(i32, u32),
    }

    check_format! { derive_default_tuple_payload
        DefaultTuplePayload::spec()
        =>
            "enum DefaultTuplePayloadType {"
            "	Alpha = 10,"
            "	Beta = 20,"
            "};"
            ""
            "union DefaultTuplePayload switch (DefaultTuplePayloadType which)"
            "{"
            "case Alpha:"
            "	unsigned int a;"
            "case Beta:"
            "	void;"
            "default:"
            "	unsigned int default;"
            "};"
            ""
    }
}

mod external_discrim {
    use super::*;

    #[derive(Debug, PartialEq, Eq, xdrs::XDR)]
    #[repr(i32)]
    enum Status {
        Ready = 10,
        Failed = 11,
    }

    #[derive(Debug, PartialEq, Eq, xdrs::XDR)]
    #[xdr(discrim_type = "Status")]
    #[repr(i32)]
    enum Message {
        Ready { value: u32 } = Status::Ready as i32,
        Failed { code: u32 } = Status::Failed as i32,
    }

    #[test]
    fn enum_external_discrim_explicit() {
        // Status::Ready = 10 = 0x0a, value = 7
        let _ready = Message::Ready { value: 7 };

        // Status::Failed = 11 = 0x0b, code = 3
        let _failed = Message::Failed { code: 3 };
    }
}
