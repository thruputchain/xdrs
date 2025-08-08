#![allow(dead_code)]

use crate::xdr::*;

pub trait HasSpecification {
    fn spec() -> Specification;
}

/*
impl<T: XDR> HasSpecification for T {
    fn spec() -> Specification {
        Specification {
            definitions: vec![T::def()]
        }
    }
}
*/

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

macro_rules! impl_declare_named {
    { $( $ty:ty => $spec:expr ),* $(,)? } =>
    {$(
        impl Declare for $ty {
            fn declare(name: Ident) -> Declaration {
                Declaration::Named {
                    name,
                    ty: $spec
                }
            }
        }
    )*};
}

impl_declare_named! {
    i32 => TypeSpec::Int,
    u32 => TypeSpec::Unsigned,
    i64 => TypeSpec::Hyper,
    u64 => TypeSpec::UnsignedHyper,
    bool => TypeSpec::Bool,
}

// TODO: use attrs to derive these project-specific typespecs?
impl_declare_named! {
    i128 => TypeSpec::Ident(Ident("int128_xdr".to_owned())),
    u128 => TypeSpec::Ident(Ident("uint128_xdr".to_owned())),
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
        let declare_named = |ty_ident|
            Declaration::Named {
                name,
                ty: TypeSpec::Ident(ty_ident)
            };

        match T::type_def() {
            TypeDef::Def(decl) => decl,
            TypeDef::Struct(ty_ident, _) => declare_named(ty_ident),
            TypeDef::Enum(ty_ident, _) => declare_named(ty_ident),
            TypeDef::Union(ty_ident, _) => declare_named(ty_ident),
        }
    }
}

// TODO: feature-gate this, as some users may not want?
impl<'a, T: HasTypeDef> HasTypeDef for &'a T {
    fn type_def() -> TypeDef {
        T::type_def()
    }
}
