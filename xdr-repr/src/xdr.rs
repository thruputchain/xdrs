//! Representation of an XDR specification

use alloc::vec::Vec;
use alloc::string::String;
use alloc::boxed::Box;

#[cfg(feature = "ns")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Namespace {
    pub name: String,
    pub spec: Specification,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Specification {
    pub definitions: Vec<Definition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Definition {
    Type(TypeDef),
    Const(ConstDef),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeDef {
    Def(Declaration),
    Enum(Ident, EnumBody),
    Struct(Ident, StructBody),
    Union(Ident, UnionBody),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstDef {
    pub ident: Ident,
    pub constant: Constant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructBody(pub Vec<Declaration>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumBody(pub Vec<(Ident, Value)>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnionBody {
    pub discriminant_decl: Declaration,
    pub case_specs: Vec<CaseSpec>, // must be non-empty
    pub default_decl: Option<Declaration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseSpec {
    pub cases: Vec<Value>, // must be non-empty
    pub decl: Declaration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Constant {
    Unsigned(u32),
    Signed(i32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Const(Constant),
    Ident(Ident),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Declaration {
    Named {
        name: Ident,
        ty: TypeSpec,
    },
    FixedOpaque {
        name: Ident,
        size: Value,
    },
    VariableOpaque {
        name: Ident,
        max: Value,
    },
    FixedArray {
        name: Ident,
        elt_ty: TypeSpec,
        len: Value,
    },
    VariableArray {
        name: Ident,
        elt_ty: TypeSpec,
        max: Value,
    },
    Optional {
        name: Ident,
        opt_ty: TypeSpec,
    },
    Void,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeSpec {
    Int,
    Unsigned,
    Hyper,
    UnsignedHyper,
    Float,
    Double,
    Quadruple,
    Bool,
    Enum(EnumBody),
    Struct(StructBody),
    Union(Box<UnionBody>),
    Ident(Ident),
}
