use crate::xdr::*;
use alloc::vec::Vec;
use alloc::vec;
use alloc::string::*;

/// Creates an identifier from a string.
pub fn ident(s: &str) -> Ident {
    Ident(s.to_string())
}

impl From<u32> for Constant {
    fn from(n: u32) -> Self {
        Self::Unsigned(n)
    }
}

impl From<i32> for Constant {
    fn from(n: i32) -> Self {
        Self::Signed(n)
    }
}

/// Creates an unsigned constant.
pub fn unsigned_constant(n: u32) -> Constant {
    Constant::Unsigned(n)
}

/// Creates a signed constant.
pub fn signed_constant(n: i32) -> Constant {
    Constant::Signed(n)
}

/// Creates an unsigned value.
pub fn unsigned(n: u32) -> Value {
    unsigned_constant(n).into()
}

/// Creates a signed value.
pub fn signed(n: i32) -> Value {
    signed_constant(n).into()
}

impl From<Constant> for Value {
    fn from(c: Constant) -> Self {
        Self::Const(c)
    }
}

impl From<Ident> for Value {
    fn from(i: Ident) -> Self {
        Self::Ident(i)
    }
}

/// Creates a value that references a named constant.
pub fn name_ref(s: &str) -> Value {
    Value::Ident(ident(s))
}

/// Creates a void declaration.
pub fn declare_void() -> Declaration {
    Declaration::Void
}

/// Creates a named type declaration.
pub fn declare_type(s: &str, type_spec: TypeSpec) -> Declaration {
    Declaration::Named {
        name: ident(s),
        ty: type_spec,
    }
}

/// Creates a fixed-length opaque data declaration.
pub fn declare_fixed_opaque(s: &str, size: u32) -> Declaration {
    Declaration::FixedOpaque {
        name: ident(s),
        size: Value::Const(size.into()),
    }
}

/// Creates an unsigned integer declaration.
pub fn declare_unsigned(s: &str) -> Declaration {
    declare_type(s, TypeSpec::Unsigned)
}

/// Creates a signed integer declaration.
pub fn declare_int(s: &str) -> Declaration {
    declare_type(s, TypeSpec::Int)
}

/// Creates a type specification that references a named type.
pub fn named_type(name: &str) -> TypeSpec {
    TypeSpec::Ident(ident(name))
}

/// Creates an inline struct type specification.
///
/// # Panics
///
/// Panics if `fields` is empty, as XDR structs must contain at least one field.
pub fn struct_type(fields: Vec<Declaration>) -> TypeSpec {
    assert!(
        !fields.is_empty(),
        "XDR struct must contain at least one field"
    );

    TypeSpec::Struct(StructBody(fields))
}

/// Creates an inline enum type specification.
pub fn enum_type(variants: Vec<(Ident, Value)>) -> TypeSpec {
    TypeSpec::Enum(EnumBody(variants))
}

/// Creates a struct type definition.
///
/// # Panics
///
/// Panics if `fields` is empty, as XDR structs must contain at least one field.
pub fn struct_def(name: &str, fields: Vec<Declaration>) -> TypeDef {
    assert!(
        !fields.is_empty(),
        "XDR struct must contain at least one field"
    );

    TypeDef::Struct(ident(name), StructBody(fields))
}

/// Creates an enum type definition.
pub fn enum_def(name: &str, variants: Vec<(Ident, Value)>) -> TypeDef {
    TypeDef::Enum(ident(name), EnumBody(variants))
}

/// Creates a union type definition.
///
/// # Panics
///
/// Panics if `case_specs` is empty, as XDR unions must have at least one case.
pub fn union_def(
    name: &str,
    discrim_name: &str,
    discrim_type: TypeSpec,
    case_specs: Vec<CaseSpec>,
    default_decl: Option<Declaration>,
) -> TypeDef {
    assert!(
        !case_specs.is_empty(),
        "XDR Union: must have at least one case-spec"
    );

    TypeDef::Union(
        ident(name),
        UnionBody {
            discriminant_decl: declare_type(discrim_name, discrim_type),
            case_specs,
            default_decl,
        },
    )
}

/// Creates a case specification with a single case value.
pub fn single_case_spec(case: &str, decl: Declaration) -> CaseSpec {
    CaseSpec {
        cases: vec![Value::Ident(ident(case))],
        decl,
    }
}

/// Creates a constant definition.
pub fn def_const(name: &str, constant: Constant) -> Definition {
    Definition::Const(ConstDef {
        ident: ident(name),
        constant,
    })
}

/// Creates an XDR specification from a list of definitions.
pub fn specification(definitions: Vec<Definition>) -> Specification {
    Specification { definitions }
}

/// Creates a namespace containing an XDR specification.
///
/// Requires the `ns` feature.
#[cfg(feature = "ns")]
#[cfg_attr(docsrs, doc(cfg(feature = "ns")))]
pub fn namespace(name: &str, spec: Specification) -> Namespace {
    Namespace {
        name: name.to_string(),
        spec,
    }
}

impl From<TypeDef> for Definition {
    fn from(type_def: TypeDef) -> Self {
        Definition::Type(type_def)
    }
}

impl From<ConstDef> for Definition {
    fn from(const_def: ConstDef) -> Self {
        Definition::Const(const_def)
    }
}

impl From<Declaration> for TypeDef {
    fn from(decl: Declaration) -> Self {
        TypeDef::Def(decl)
    }
}

impl From<Declaration> for Definition {
    fn from(decl: Declaration) -> Self {
        Into::<TypeDef>::into(decl).into()
    }
}
