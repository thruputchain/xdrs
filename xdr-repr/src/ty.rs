//! Traits for describing XDR type representations.
//!
//! This module provides traits that allow Rust types to describe their
//! XDR representation, enabling automatic generation of XDR specifications.

#![allow(dead_code)]

use crate::xdr::*;
use alloc::string::*;

/// Types that can produce a complete XDR specification.
pub trait HasSpecification {
    /// Returns the complete XDR specification for this type.
    fn spec() -> Specification;
}

/// Types that can produce an XDR definition.
///
/// This trait is automatically implemented for types that implement [`HasTypeDef`].
pub trait XDR {
    /// Returns the XDR definition for this type.
    fn def() -> Definition;
}

impl<T: HasTypeDef> XDR for T {
    fn def() -> Definition {
        Definition::Type(T::type_def())
    }
}

/// Types that can describe their XDR type definition.
///
/// This trait allows types to specify their complete XDR type definition,
/// including structs, enums, unions, and typedefs.
pub trait HasTypeDef {
    /// Returns the XDR type definition for this type.
    fn type_def() -> TypeDef;
}

/// Types that can describe their XDR type specification.
///
/// This trait allows types to specify their XDR type specification,
/// which is used in declarations and type references.
pub trait HasTypeSpec {
    /// Returns the XDR type specification for this type.
    fn type_spec() -> TypeSpec;
}

impl<T: HasTypeDef> HasTypeSpec for T {
    fn type_spec() -> TypeSpec {
        let ident = type_def_ident(T::type_def());
        TypeSpec::Ident(ident.clone())
    }
}

pub(crate) fn type_def_ident(type_def: TypeDef) -> Ident {
    match type_def {
        TypeDef::Def(decl) => decl_ident(decl),
        TypeDef::Enum(ident, _) => ident,
        TypeDef::Struct(ident, _) => ident,
        TypeDef::Union(ident, _) => ident,
    }
}

fn decl_ident(decl: Declaration) -> Ident {
    match decl {
        Declaration::Named { name: ident, .. } => ident,
        Declaration::FixedOpaque { name: ident, .. } => ident,
        Declaration::VariableOpaque { name: ident, .. } => ident,
        Declaration::String { name: ident, .. } => ident,
        Declaration::FixedArray { name: ident, .. } => ident,
        Declaration::VariableArray { name: ident, .. } => ident,
        Declaration::Optional { name: ident, .. } => ident,
        Declaration::Void => Ident("void".to_string()),
    }
}

macro_rules! impl_has_type_spec {
    { $( $ty:ty => $spec:expr ),* $(,)? } =>
    {$(
        impl HasTypeSpec for $ty {
            fn type_spec() -> TypeSpec {
                $spec
            }
        }
    )*};
}

impl_has_type_spec! {
    i32 => TypeSpec::Int,
    u32 => TypeSpec::Unsigned,
    i64 => TypeSpec::Hyper,
    u64 => TypeSpec::UnsignedHyper,
    bool => TypeSpec::Bool,
}

// 128-bit integer types using the alternate XDR encoding.
// When the `int128_xdr` feature is enabled, `i128` and `u128` are represented
// as named types referring to double-u64 structures defined outside this package.
#[cfg(feature = "int128_xdr")]
#[cfg_attr(docsrs, doc(cfg(feature = "int128_xdr")))]
// These refer to double-u64 structures defined outside this package
impl_has_type_spec! {
    i128 => TypeSpec::Ident(Ident("int128_xdr".to_string())),
    u128 => TypeSpec::Ident(Ident("uint128_xdr".to_string())),
}

/// Types that can create an XDR declaration with a given name.
///
/// This trait allows types to create a declaration that can be used
/// in struct fields, procedure parameters, etc.
pub trait Declare {
    /// Creates an XDR declaration with the given name.
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

#[cfg(feature = "int128_xdr")]
#[cfg_attr(docsrs, doc(cfg(feature = "int128_xdr")))]
// These refer to double-u64 structures defined outside this package
impl_declare_named! {
    i128 => TypeSpec::Ident(Ident("int128_xdr".to_string())),
    u128 => TypeSpec::Ident(Ident("uint128_xdr".to_string())),
}

impl<T: HasTypeSpec> Declare for Option<T> {
    fn declare(name: Ident) -> Declaration {
        Declaration::Optional {
            name,
            opt_ty: T::type_spec(),
        }
    }
}

const _: () = {
    use alloc::boxed::Box;

    impl<T :HasTypeDef> HasTypeDef for Box<T> {
        fn type_def() -> TypeDef {
            T::type_def()
        }
    }
};

impl<const N: usize> Declare for [u8; N] {
    fn declare(name: Ident) -> Declaration {
        Declaration::FixedOpaque {
            name,
            // TODO: use compile-time asserts to check N fits in u32
            size: Value::Const((N as u32).into()),
        }
    }
}

impl<const N: usize, T: HasTypeSpec> Declare for [T; N] {
    fn declare(name: Ident) -> Declaration {
        Declaration::FixedArray {
            name,
            elt_ty: T::type_spec(),
            // TODO: use compile-time asserts to check N fits in u32
            len: Value::Const((N as u32).into()),
        }
    }
}

impl Declare for &[u8] {
    fn declare(name: Ident) -> Declaration {
        Declaration::VariableOpaque {
            name,
            max: Value::Const(Constant::Unsigned(u32::MAX)),
        }
    }
}

impl<T: HasTypeSpec> Declare for &[T] {
    fn declare(name: Ident) -> Declaration {
        Declaration::VariableArray {
            name,
            elt_ty: T::type_spec(),
            max: Value::Const(Constant::Unsigned(u32::MAX)),
        }
    }
}

#[cfg(feature = "xdr_common")]
const _: () = {
    use xdr_common::{BytesM, VecM, StringM};

    #[cfg_attr(docsrs, doc(cfg(feature = "xdr_common")))]
    impl<T, const MAX: u32> Declare for VecM<T, MAX>
    where
        T: HasTypeSpec,
    {
        fn declare(name: Ident) -> Declaration {
            Declaration::VariableArray {
                name,
                elt_ty: T::type_spec(),
                // TODO: use compile-time asserts to check MAX fits in u32
                max: Value::Const(Constant::Unsigned(MAX)),
            }
        }
    }

    #[cfg_attr(docsrs, doc(cfg(feature = "xdr_common")))]
    impl<const MAX: u32> Declare for BytesM<MAX> {
        fn declare(name: Ident) -> Declaration {
            Declaration::VariableOpaque {
                name,
                max: Value::Const(Constant::Unsigned(MAX)),
            }
        }
    }

    impl<const MAX: u32> Declare for StringM<MAX> {
        fn declare(name: Ident) -> Declaration {
            Declaration::String {
                name,
                max: Value::Const(Constant::Unsigned(MAX)),
            }
        }
    }
};

const _: () = {
    use alloc::vec::Vec;

    impl Declare for Vec<u8> {
        fn declare(name: Ident) -> Declaration {
            Declaration::VariableOpaque {
                name,
                max: Value::Const(Constant::Unsigned(u32::MAX)),
            }
        }
    }
};

impl HasTypeDef for () {
    fn type_def() -> TypeDef {
        TypeDef::Def(Declaration::Void)
    }
}

impl<T> Declare for T
where
    T: HasTypeDef,
{
    fn declare(name: Ident) -> Declaration {
        let declare_named = |ty_ident| Declaration::Named {
            name,
            ty: TypeSpec::Ident(ty_ident),
        };

        match T::type_def() {
            TypeDef::Def(decl) => decl,
            TypeDef::Struct(ty_ident, _) => declare_named(ty_ident),
            TypeDef::Enum(ty_ident, _) => declare_named(ty_ident),
            TypeDef::Union(ty_ident, _) => declare_named(ty_ident),
        }
    }
}

// TODO: feature-gate this, as some users may not want it?
impl<T: HasTypeDef> HasTypeDef for &T {
    fn type_def() -> TypeDef {
        T::type_def()
    }
}


