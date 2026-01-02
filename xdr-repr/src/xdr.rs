//! Representation of an XDR specification

use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;
use alloc::boxed::Box;

#[cfg(feature = "ns")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Namespace {
    pub name: Option<String>,
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
pub struct Constant(pub i128);

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
    FixedArray {
        name: Ident,
        elt_ty: TypeSpec,
        len: Value,
    },
    FixedOpaque {
        name: Ident,
        size: Value,
    },
    VariableArray {
        name: Ident,
        elt_ty: TypeSpec,
        max: Value,
    },
    VariableOpaque {
        name: Ident,
        max: Value,
    },
    VariableString {
        name: Ident,
        max: Value,
    },
    Optional {
        name: Ident,
        opt_ty: TypeSpec,
    },
    Void,
}

impl Declaration {
    pub fn get_type_json(&self) -> String {
        use alloc::format;
        match self {
            Declaration::Named { name: _, ty } => {
                format!("{{ \"type\": \"{}\" }}", ty.get_type_json())
            },
            Declaration::FixedOpaque { name: _, size } => {
                format!("{{ \"type\": \"fixed opaque\", \"len\": {} }}", size)
            },
            Declaration::FixedArray { name: _, elt_ty, len } => {
                format!("{{ \"type\": \"fixed array\", \"elt_ty\": {}, \"len\": {} }}", elt_ty.get_type_json(), len)
            },
            Declaration::VariableString { name: _, max } => {
                format!("{{ \"type\": \"variable string\", \"max\": {} }}", max)
            },
            Declaration::VariableOpaque { name: _, max } => {
                format!("{{ \"type\": \"variable opaque\", \"max\": {} }}", max)
            },
            Declaration::VariableArray { name: _, elt_ty, max } => {
                format!("{{ \"type\": \"variable array\", \"elt_ty\": {}, \"max\": {} }}", elt_ty.get_type_json(), max)
            },
            Declaration::Optional { name: _, opt_ty } => {
                format!("{{ \"type\": \"optional\", \"opt_ty\": {} }}", opt_ty.get_type_json())
            },
            Declaration::Void => {
                format!("{{ \"type\": \"void\" }}")
            }
        }
    }
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

impl TypeSpec {
    pub fn get_type_json(&self) -> alloc::string::String {
        match self {
            TypeSpec::Int => "int".to_string(),
            TypeSpec::Unsigned => "unsigned".to_string(),
            TypeSpec::Hyper => "hyper".to_string(),
            TypeSpec::UnsignedHyper => "unsigned hyper".to_string(),
            TypeSpec::Float => "float".to_string(),
            TypeSpec::Double => "double".to_string(),
            TypeSpec::Quadruple => "quadruple".to_string(),
            TypeSpec::Bool => "bool".to_string(),
            TypeSpec::Enum(_body) => panic!("Embedded enum type not supported"),
            TypeSpec::Struct(_body) => panic!("Embedded struct type not supported"),
            TypeSpec::Union(_body) => panic!("Embedded union type not supported"),
            TypeSpec::Ident(ident) => ident.0.clone(),
        }
    }
}
