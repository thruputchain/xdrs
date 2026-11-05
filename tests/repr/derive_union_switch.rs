//! `derive(XDR)` for unions that name their switch type with
//! `#[xdr(discriminant = "...")]`: the switch is declared with that type's
//! own type specification, and each case is labelled with a value of it.

use xdr_repr::*;
use xdrs::*;

/// The switch declaration and the case labels of a union's definition, and
/// whether it has a `default:` arm.
fn switch_of(def: TypeDef) -> (Declaration, Vec<Vec<Value>>, bool) {
    let TypeDef::Union(_, body) = def else {
        panic!("not a union: {def:?}");
    };
    let cases = body.case_specs.into_iter().map(|cs| cs.cases).collect();
    (body.discriminant_decl, cases, body.default_decl.is_some())
}

fn switch_decl(ty: TypeSpec) -> Declaration {
    Declaration::Named {
        name: ident("which"),
        ty,
    }
}

pub mod unsigned {
    use super::*;

    #[derive(XDR, ReadXDR, WriteXDR)]
    #[xdr(discriminant = "u32")]
    #[repr(i32)]
    pub enum Unsigned {
        Small(i32) = 32,
        Big { flag: bool } = 2_147_483_648u32 as i32,
        Max = 4_294_967_295u32 as i32,
    }

    #[test]
    fn switch_and_labels() {
        assert_eq!(
            switch_of(Unsigned::type_def()),
            (
                switch_decl(TypeSpec::Unsigned),
                vec![
                    vec![unsigned(32)],
                    vec![unsigned(2_147_483_648)],
                    vec![unsigned(4_294_967_295)],
                ],
                false,
            )
        );
    }

    // No `UnsignedType` enum is generated.
    check_format! { spec
        Unsigned::spec()
        =>
            "union Unsigned switch (unsigned int which)"
            "{"
            "case 32:"
            "	int small;"
            "case 2147483648:"
            "	bool flag;"
            "case 4294967295:"
            "	void;"
            "};"
            ""
    }

    // The schema and the encoders agree: CXS writes an unsigned switch.
    #[test]
    fn encodes_unsigned() {
        assert_eq!(
            cxs::encode(&Unsigned::Max).unwrap(),
            cxs::encode(&u32::MAX).unwrap()
        );
        let small = cxs::encode(&Unsigned::Small(-1)).unwrap();
        let tag = cxs::encode(&32u32).unwrap();
        assert_eq!(&small[..tag.len()], &tag[..]);
    }
}

// A union whose only arm is `default:` still declares its switch type.
pub mod only_default {
    use super::*;

    #[derive(XDR, ReadXDR, WriteXDR)]
    #[xdr(discriminant = "u32")]
    #[repr(i32)]
    pub enum OnlyDefault {
        #[xdr(default_variant)]
        Other(u32, i32),
    }

    check_format! { spec
        OnlyDefault::spec()
        =>
            "union OnlyDefault switch (unsigned int which)"
            "{"
            "default:"
            "	int other;"
            "};"
            ""
    }
}

pub mod signed {
    use super::*;

    #[derive(XDR)]
    #[xdr(discriminant = "i32", discrim_var = "k")]
    #[repr(i32)]
    pub enum Signed {
        Neg(u32) = -1,
        Zero = 0,
        #[xdr(name = "Renamed")]
        One(i32) = 1,
    }

    #[test]
    fn switch_and_labels() {
        let (decl, cases, has_default) = switch_of(Signed::type_def());
        assert_eq!(
            decl,
            Declaration::Named {
                name: ident("k"),
                ty: TypeSpec::Int
            }
        );
        assert_eq!(
            cases,
            vec![vec![signed(-1)], vec![signed(0)], vec![signed(1)]]
        );
        assert!(!has_default);
    }

    check_format! { spec
        Signed::spec()
        =>
            "union Signed switch (int k)"
            "{"
            "case -1:"
            "	unsigned int neg;"
            "case 0:"
            "	void;"
            "case 1:"
            "	int one;"
            "};"
            ""
    }

    // Implicit discriminants are the variants' indices, as for the encoders.
    #[derive(XDR)]
    #[xdr(discriminant = "i32")]
    #[repr(i32)]
    pub enum Implicit {
        A(u32),
        B,
    }

    check_format! { implicit
        Implicit::def()
        =>
            "union Implicit switch (int which)"
            "{"
            "case 0:"
            "	unsigned int a;"
            "case 1:"
            "	void;"
            "};"
    }
}

pub mod boolean {
    use super::*;

    #[derive(XDR, ReadXDR, WriteXDR)]
    #[xdr(discriminant = "bool")]
    #[repr(i32)]
    pub enum Both {
        No = 0,
        Yes(u32) = 1,
    }

    #[test]
    fn switch_and_labels() {
        assert_eq!(
            switch_of(Both::type_def()),
            (
                switch_decl(TypeSpec::Bool),
                vec![vec![name_ref("FALSE")], vec![name_ref("TRUE")]],
                false,
            )
        );
    }

    check_format! { spec
        Both::spec()
        =>
            "union Both switch (bool which)"
            "{"
            "case FALSE:"
            "	void;"
            "case TRUE:"
            "	unsigned int yes;"
            "};"
            ""
    }

    // `case TRUE: ...; default: ...`, where the default means FALSE.
    #[derive(XDR, ReadXDR, WriteXDR)]
    #[xdr(discriminant = "bool")]
    #[repr(i32)]
    pub enum WithDefault {
        Yes(u32) = 1,
        #[xdr(default_variant)]
        Other(bool, i32) = 2,
    }

    #[test]
    fn default_switch_and_labels() {
        assert_eq!(
            switch_of(WithDefault::type_def()),
            (
                switch_decl(TypeSpec::Bool),
                vec![vec![name_ref("TRUE")]],
                true,
            )
        );
    }

    check_format! { default_spec
        WithDefault::spec()
        =>
            "union WithDefault switch (bool which)"
            "{"
            "case TRUE:"
            "	unsigned int yes;"
            "default:"
            "	int other;"
            "};"
            ""
    }

    // A bool tag other than 0 or 1 has no label.
    #[derive(XDR)]
    #[xdr(discriminant = "bool")]
    #[repr(i32)]
    pub enum BadTag {
        Two(u32) = 2,
    }

    #[test]
    #[should_panic(expected = "neither FALSE (0) nor TRUE (1)")]
    fn bad_tag() {
        let _ = BadTag::type_def();
    }
}

pub mod enum_switch {
    use super::*;

    // The members' XDR names differ from the Rust variant names, and from
    // the union's variant names: the labels come from the enum's own
    // definition.
    #[derive(XDR)]
    #[xdr(name = "Color")]
    #[repr(i32)]
    pub enum Colour {
        #[xdr(name = "RED")]
        Red = 0,
        #[xdr(name = "GREEN")]
        Green = 1,
        #[xdr(name = "BLUE")]
        Blue = -2,
    }

    #[derive(XDR)]
    #[xdr(discriminant = "Colour")]
    #[repr(i32)]
    pub enum Paint {
        Warm(u32) = Colour::Red as i32,
        Cold {
            depth: i32,
        } = Colour::Blue as i32,
        #[xdr(default_variant)]
        Other(Colour) = 100,
    }

    #[test]
    fn switch_and_labels() {
        assert_eq!(
            switch_of(Paint::type_def()),
            (
                switch_decl(named_type("Color")),
                vec![vec![name_ref("RED")], vec![name_ref("BLUE")]],
                true,
            )
        );
    }

    // Only the union: the switch type is defined on its own.
    check_format! { spec
        Paint::spec()
        =>
            "union Paint switch (Color which)"
            "{"
            "case RED:"
            "	unsigned int warm;"
            "case BLUE:"
            "	int depth;"
            "default:"
            "	void;"
            "};"
            ""
    }

    // A tag that is not a member's value has no label.
    #[derive(XDR)]
    #[xdr(discriminant = "Colour")]
    #[repr(i32)]
    pub enum Stray {
        Seven(u32) = 7,
    }

    #[test]
    #[should_panic(expected = "no enum member has the literal value 7")]
    fn stray_tag() {
        let _ = Stray::type_def();
    }

    // A switch type that is not an enum has no labels.
    #[derive(XDR)]
    pub struct NotAnEnum {
        pub x: i32,
    }

    #[derive(XDR)]
    #[xdr(discriminant = "NotAnEnum")]
    #[repr(i32)]
    pub enum OnStruct {
        A(u32) = 0,
    }

    #[test]
    #[should_panic(expected = "switch type NotAnEnum is not an enum")]
    fn struct_switch() {
        let _ = OnStruct::type_def();
    }
}

pub mod alias {
    use super::*;

    pub type Index = u32;

    mod elsewhere {
        pub type Flag = bool;
    }

    #[derive(XDR)]
    #[xdr(discriminant = "Index")]
    #[repr(i32)]
    pub enum ByIndex {
        First(i32) = 0,
        Last(i32) = 4_294_967_295u32 as i32,
    }

    #[test]
    fn switch_and_labels() {
        assert_eq!(
            switch_of(ByIndex::type_def()),
            (
                switch_decl(TypeSpec::Unsigned),
                vec![vec![unsigned(0)], vec![unsigned(4_294_967_295)]],
                false,
            )
        );
    }

    check_format! { spec
        ByIndex::def()
        =>
            "union ByIndex switch (unsigned int which)"
            "{"
            "case 0:"
            "	int first;"
            "case 4294967295:"
            "	int last;"
            "};"
    }

    #[derive(XDR)]
    #[xdr(discriminant = "elsewhere::Flag")]
    #[repr(i32)]
    pub enum ByFlag {
        On(u32) = 1,
    }

    check_format! { path_spec
        ByFlag::def()
        =>
            "union ByFlag switch (bool which)"
            "{"
            "case TRUE:"
            "	unsigned int on;"
            "};"
    }
}

// A fieldless enum that names a switch type is a union of void arms, not
// an XDR enum.
pub mod fieldless {
    use super::*;

    #[derive(XDR)]
    #[xdr(discriminant = "u32")]
    #[repr(i32)]
    pub enum Voids {
        A = 1,
        B = 4_294_967_294u32 as i32,
    }

    check_format! { spec
        Voids::spec()
        =>
            "union Voids switch (unsigned int which)"
            "{"
            "case 1:"
            "	void;"
            "case 4294967294:"
            "	void;"
            "};"
            ""
    }
}

// Without the attribute, a union is switched on a generated `{Name}Type`
// enum and labelled by variant name, as before.
pub mod unannotated {
    use super::*;

    #[derive(XDR)]
    #[repr(i32)]
    pub enum Plain {
        Small(i32) = 32,
        Big = 7,
        #[xdr(default_variant)]
        Other(i32) = 8,
    }

    #[test]
    fn switch_and_labels() {
        assert_eq!(
            switch_of(Plain::type_def()),
            (
                switch_decl(named_type("PlainType")),
                vec![vec![name_ref("Small")], vec![name_ref("Big")]],
                true,
            )
        );
    }

    check_format! { spec
        Plain::spec()
        =>
            "enum PlainType {"
            "	Small = 32,"
            "	Big = 7,"
            "};"
            ""
            "union Plain switch (PlainType which)"
            "{"
            "case Small:"
            "	int small;"
            "case Big:"
            "	void;"
            "default:"
            "	void;"
            "};"
            ""
    }
}
