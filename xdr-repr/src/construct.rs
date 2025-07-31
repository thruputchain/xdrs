use crate::xdr::*;

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

pub fn name_ref(s: &str) -> Value {
    Value::Ident(ident(s))
}

pub fn declare_type(s: &str, type_spec: TypeSpec) -> Declaration {
    Declaration::Named {
        name: ident(s),
        ty: type_spec,
    }
}

pub fn declare_fixed_opaque(s: &str, size: u32) -> Declaration {
    Declaration::FixedOpaque {
        name: ident(s),
        size: Value::Const(size.into()),
    }
}

pub fn declare_unsigned(s: &str) -> Declaration {
    declare_type(s, TypeSpec::UnsignedInt)
}

pub fn declare_int(s: &str) -> Declaration {
    declare_type(s, TypeSpec::Int)
}

pub fn declare_named_type(s: &str, ident: Ident) -> Declaration {
    declare_type(s, TypeSpec::Ident(ident))
}

pub fn struct_type(fields: Vec<Declaration>) -> TypeSpec {
    TypeSpec::Struct(
        StructBody(fields))
}

pub fn struct_def(name: &str, fields: Vec<Declaration>) -> TypeDef {
    TypeDef::Struct(ident(name), StructBody(fields))
}

pub fn def_struct(name: &str, fields: Vec<Declaration>) -> Definition {
    Definition::Type(
        TypeDef::Struct(ident(name), StructBody(fields)))
}

pub fn def_const(name: &str, constant: Constant) -> Definition {
    Definition::Const(
        ConstDef { ident: ident(name), constant })
}

pub fn spec(definitions: Vec<Definition>) -> Specification {
    Specification { definitions }
}

#[macro_export]
macro_rules! spec {
    { $($def:expr),* $(,)? } => {
        spec(vec![$($def),*])
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
