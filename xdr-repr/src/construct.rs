use crate::xdr::*;
use alloc::vec::Vec;
use alloc::vec;
use alloc::string::*;

use super::keywords::is_keyword;

pub fn ident(s: &str) -> Ident {
    if is_keyword(s) {
        panic!("Can't construct identifier from keyword string \"{}\"", s);
    }

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

pub fn unsigned_constant(n: u32) -> Constant {
    Constant::Unsigned(n)
}

pub fn signed_constant(n: i32) -> Constant {
    Constant::Signed(n)
}

pub fn unsigned(n: u32) -> Value {
    unsigned_constant(n).into()
}

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

pub fn name_ref(name: &str) -> Value {
    Value::Ident(ident(name))
}

pub fn declare_void() -> Declaration {
    Declaration::Void
}

pub fn declare_type(name: &str, type_spec: TypeSpec) -> Declaration {
    Declaration::Named {
        name: ident(name),
        ty: type_spec,
    }
}

pub fn declare_variable_array(name: &str, elt_ty: TypeSpec, max: Value) -> Declaration {
    Declaration::VariableArray {
        name: ident(name),
        elt_ty,
        max,
    }
}

pub fn declare_variable_opaque(name: &str, max: Value) -> Declaration {
    Declaration::VariableOpaque {
        name: ident(name),
        max,
    }
}

pub fn declare_variable_string(name: &str, max: Value) -> Declaration {
    Declaration::VariableString {
        name: ident(name),
        max,
    }
}

pub fn declare_fixed_array(name: &str, elt_ty: TypeSpec, len: Value) -> Declaration {
    Declaration::FixedArray {
        name: ident(name),
        elt_ty,
        len,
    }
}

pub fn declare_fixed_opaque(name: &str, size: u32) -> Declaration {
    Declaration::FixedOpaque {
        name: ident(name),
        size: Value::Const(size.into()),
    }
}

pub fn declare_optional(name: &str, opt_ty: TypeSpec) -> Declaration {
    Declaration::Optional {
        name: ident(name),
        opt_ty,
    }
}

pub fn declare_unsigned(s: &str) -> Declaration {
    declare_type(s, TypeSpec::Unsigned)
}

pub fn declare_int(s: &str) -> Declaration {
    declare_type(s, TypeSpec::Int)
}

pub const XDR_I32: TypeSpec = TypeSpec::Int;
pub const XDR_U32: TypeSpec = TypeSpec::Unsigned;
pub const XDR_I64: TypeSpec = TypeSpec::Hyper;
pub const XDR_U64: TypeSpec = TypeSpec::UnsignedHyper;
pub const XDR_FLOAT: TypeSpec = TypeSpec::Float;
pub const XDR_DOUBLE: TypeSpec = TypeSpec::Double;
pub const XDR_QUAD: TypeSpec = TypeSpec::Quadruple;
pub const XDR_BOOL: TypeSpec = TypeSpec::Bool;

pub fn named_type(name: &str) -> TypeSpec {
    TypeSpec::Ident(ident(name))
}

pub fn struct_type(fields: Vec<Declaration>) -> TypeSpec {
    TypeSpec::Struct(StructBody(fields))
}

pub fn enum_type(variants: Vec<(Ident, Value)>) -> TypeSpec {
    TypeSpec::Enum(EnumBody(variants))
}

pub fn def(decl: Declaration) -> TypeDef {
    TypeDef::Def(decl)
}

pub fn struct_def(name: &str, fields: Vec<Declaration>) -> TypeDef {
    TypeDef::Struct(ident(name), StructBody(fields))
}

pub fn enum_def(name: &str, variants: Vec<(Ident, Value)>) -> TypeDef {
    TypeDef::Enum(ident(name), EnumBody(variants))
}

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

pub fn single_case_spec(case: &str, decl: Declaration) -> CaseSpec {
    CaseSpec {
        cases: vec![Value::Ident(ident(case))],
        decl,
    }
}

pub fn case_spec(case: Value, decl: Declaration) -> CaseSpec {
    CaseSpec { cases: vec![case], decl }
}

pub fn def_const(name: &str, constant: Constant) -> Definition {
    Definition::Const(ConstDef {
        ident: ident(name),
        constant,
    })
}

pub fn def_type(type_def: TypeDef) -> Definition {
    Definition::Type(type_def)
}

pub fn specification(definitions: Vec<Definition>) -> Specification {
    Specification { definitions }
}

#[cfg(feature = "ns")]
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
