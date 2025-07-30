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
pub struct Constant(pub usize);

#[derive(Debug, Clone)]
pub enum Value {
    Const(Constant),
    Ident(Ident),
}

#[derive(Debug, Clone)]
pub enum Declaration {
    Named(Ident, TypeSpec),
    FixedOpaque(Ident, Value),
    Void,
}

#[derive(Debug, Clone)]
pub enum TypeSpec {
    Int,
    UnsignedInt,
    Ident(Ident),
}
