//! Representation of an XDR specification

#[derive(Debug, Clone)]
pub struct Specification {
    pub definitions: Vec<Definition>,
}

#[derive(Debug, Clone)]
pub enum Definition {
    Type(TypeDef),
    Const(ConstDef),
}

#[derive(Debug, Clone)]
pub enum TypeDef {
    Def(Declaration),

    // We avoid these in favor of `TypeSpec::Struct`
    // Enum(Ident, EnumBody),
    Struct(Ident, StructBody),
    // Union(Ident, UnionBody)
}

#[derive(Debug, Clone)]
pub struct ConstDef {
    pub ident: Ident,
    pub constant: Constant,
}

#[derive(Debug, Clone)]
pub struct Ident(pub String);

#[derive(Debug, Clone)]
pub struct StructBody(pub Vec<Declaration>);

// struct EnumBody(pub Vec<(Ident, Value)>);

// struct UnionBody(...);

#[derive(Debug, Clone)]
pub enum Constant {
    Unsigned(u32),
    Signed(i32),
}

#[derive(Debug, Clone)]
pub enum Value {
    Const(Constant),
    Ident(Ident),
}

#[derive(Debug, Clone)]
pub enum Declaration {
    Named { name: Ident, ty: TypeSpec },
    FixedOpaque { name: Ident, size: Value },
    VariableOpaque { name: Ident, max: Value },
    FixedArray { name: Ident, elt_ty: TypeSpec, len: Value },
    VariableArray { name: Ident, elt_ty: TypeSpec, max: Value },
    Optional { name: Ident, opt_ty: TypeSpec },
    Void,
}

#[derive(Debug, Clone)]
pub enum TypeSpec {
    Int,
    UnsignedInt,
    Hyper,
    UnsignedHyper,
    Float,
    Double,
    Quadruple,
    Bool,
    // Enum(EnumBody),
    Struct(StructBody),
    // Union(UnionBody),
    Ident(Ident),
}
