#![allow(dead_code)]

use crate::xdr::*;

pub trait HasSpecification {
    fn spec() -> Specification;
}

impl<T: XDR> HasSpecification for T {
    fn spec() -> Specification {
        Specification {
            definitions: vec![T::def()]
        }
    }
}

pub trait XDR {
    fn def() -> Definition;
}

pub trait HasTypeDef {
    fn type_def() -> TypeDef;
}

impl<T: HasTypeDef> XDR for T {
    fn def() -> Definition {
        Definition::Type(T::type_def())
    }
}

pub trait Declare {
    fn declare(name: Ident) -> Declaration;
}

impl Declare for u32 {
    fn declare(name: Ident) -> Declaration {
        Declaration::Named {
            name,
            ty: TypeSpec::Unsigned,
        }
    }
}

impl<const N: usize> Declare for [u8; N] {
    fn declare(name: Ident) -> Declaration {
        Declaration::FixedOpaque {
            name,
            // TODO: use compile-time asserts to check N fits in u32
            size: Value::Const((N as u32).into()),
        }
    }
}

#[cfg(feature = "xdr_common")]
const _: () = {
    use xdr_common::BytesM;

    impl<const MAX: u32> Declare for BytesM<MAX> {
        fn declare(name: Ident) -> Declaration {
            Declaration::VariableOpaque {
                name,
                // TODO: use compile-time asserts to check MAX fits in u32
                max: Value::Const(Constant::Unsigned(MAX as u32))
            }
        }
    }
};

impl<T> Declare for T
    where T: HasTypeDef
{
    fn declare(name: Ident) -> Declaration {
        match T::type_def() {
            TypeDef::Def(decl) => decl,
            TypeDef::Struct(ty_ident, _) =>
                Declaration::Named {
                    name,
                    ty: TypeSpec::Ident(ty_ident)
                },
            TypeDef::Enum(ty_ident, _) =>
                Declaration::Named {
                    name,
                    ty: TypeSpec::Ident(ty_ident)
                },
            // Union(Ident, UnionBody)
        }
    }
}
