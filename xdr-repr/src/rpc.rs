//! Representation of a SunRPC program definition

pub use super::xdr::TypeSpec;

pub struct Id(pub String);

pub struct Constant(pub usize);

pub struct Program {
    pub id: Id,
    pub value: Constant,
    pub versions: Vec<Version>,
}

pub struct Version {
    pub id: Id,
    pub value: Constant,
    pub procedures: Vec<Procedure>,
}

pub struct Procedure {
    pub id: Id,
    pub value: Constant,
    pub return_type: Option<TypeSpec>,
    pub arg_types: Vec<TypeSpec>,
}
