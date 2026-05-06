//! Traits for describing XDR type representations.
//!
//! This module provides traits that allow Rust types to describe their XDR
//! representation, enabling automatic generation of XDR specifications.

#![allow(dead_code)]

use crate::ident;
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
/// This trait allows types to specify their XDR type definition, including
/// structs, enums, unions, and typedefs.
pub trait HasTypeDef {
    /// Returns the XDR type definition for this type.
    fn type_def() -> TypeDef;
}

/// Types that can describe their XDR type specification.
///
/// This trait allows types to specify their XDR type specification, which is
/// used in declarations and type references.
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
// When the `int128` feature is enabled, `i128` and `u128` are represented
// as named types referring to double-u64 structures defined outside this package.
#[cfg(feature = "int128")]
#[cfg_attr(docsrs, doc(cfg(feature = "int128")))]
// These refer to double-u64 structures defined outside this package
impl_has_type_spec! {
    i128 => <(i64, u64)>::type_spec(),
    u128 => <(u64, u64)>::type_spec()
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

#[cfg(feature = "int128")]
#[cfg_attr(docsrs, doc(cfg(feature = "int128")))]
// These refer to double-u64 structures defined outside this package
impl_declare_named! {
    i128 => <(i64, u64)>::type_spec(),
    u128 => <(u64, u64)>::type_spec(),
}

impl<T: HasTypeSpec> HasTypeSpec for Option<T> {
    fn type_spec() -> TypeSpec {
        // Options aren't types in XDR, so we represent them as embedded structs
        // with a single optional field named "unnamed_0"
        TypeSpec::Struct(StructBody(alloc::vec![Declaration::Optional {
            name: ident("unnamed_0"),
            opt_ty: T::type_spec(),
        }]))
    }
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

    impl<T: HasTypeDef> HasTypeDef for Box<T> {
        fn type_def() -> TypeDef {
            T::type_def()
        }
    }
};

impl<const N: usize> Declare for [u8; N] {
    fn declare(name: Ident) -> Declaration {
        const { assert!(N <= u32::MAX as usize, "N exceeds u32::MAX") }
        Declaration::FixedOpaque {
            name,
            size: Value::Const((N as u32).into()),
        }
    }
}

// HasTypeSpec for [u8; N] - returns embedded struct with fixed opaque field
// This allows nested arrays like [[u8; 3]; 5] to work
impl<const N: usize> HasTypeSpec for [u8; N] {
    fn type_spec() -> TypeSpec {
        const { assert!(N <= u32::MAX as usize, "N exceeds u32::MAX") }
        // Arrays aren't types in XDR, so we represent them as embedded structs
        // with a single field named "unnamed_0"
        TypeSpec::Struct(StructBody(alloc::vec![Declaration::FixedOpaque {
            name: ident("unnamed_0"),
            size: Value::Const((N as u32).into()),
        }]))
    }
}

impl<const N: usize, T: HasTypeSpec> Declare for [T; N] {
    fn declare(name: Ident) -> Declaration {
        const { assert!(N <= u32::MAX as usize, "N exceeds u32::MAX") }
        Declaration::FixedArray {
            name,
            elt_ty: T::type_spec(),
            len: Value::Const((N as u32).into()),
        }
    }
}

// HasTypeSpec for [T; N] - returns embedded struct with fixed array field
// This allows nested arrays like [[u32; 3]; 5] to work
impl<const N: usize, T: HasTypeSpec> HasTypeSpec for [T; N] {
    fn type_spec() -> TypeSpec {
        const { assert!(N <= u32::MAX as usize, "N exceeds u32::MAX") }
        // Arrays aren't types in XDR, so we represent them as embedded structs
        // with a single field named "unnamed_0"
        TypeSpec::Struct(StructBody(alloc::vec![Declaration::FixedArray {
            name: ident("unnamed_0"),
            elt_ty: T::type_spec(),
            len: Value::Const((N as u32).into()),
        }]))
    }
}

impl HasTypeSpec for &[u8] {
    fn type_spec() -> TypeSpec {
        TypeSpec::Struct(StructBody(alloc::vec![Declaration::VariableOpaque {
            name: ident("unnamed_0"),
            max: Value::Const(Constant::Unsigned(u32::MAX)),
        }]))
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

const _: () = {
    use xdr_common::{BytesM, StaticIfNoAlloc, StringM, VecM};

    impl<T, const MAX: u32> Declare for VecM<T, MAX>
    where
        T: HasTypeSpec + StaticIfNoAlloc,
    {
        fn declare(name: Ident) -> Declaration {
            Declaration::VariableArray {
                name,
                elt_ty: T::type_spec(),
                max: Value::Const(Constant::Unsigned(MAX)),
            }
        }
    }

    /// We default a `VecM<u8>` to declaring as a BytesM;
    /// It's not clear to an end user what the difference is between a `VecM<u8>` and a BytesM,
    /// so this makes the two types work more like each other.
    impl<const MAX: u32> HasTypeSpec for VecM<u8, MAX> {
        fn type_spec() -> TypeSpec {
            BytesM::<MAX>::type_spec()
        }
    }

    impl<const MAX: u32> Declare for VecM<u8, MAX> {
        fn declare(name: Ident) -> Declaration {
            BytesM::<MAX>::declare(name)
        }
    }

    impl<const MAX: u32> HasTypeSpec for BytesM<MAX> {
        fn type_spec() -> TypeSpec {
            TypeSpec::Struct(StructBody(alloc::vec![Declaration::VariableOpaque {
                name: ident("unnamed_0"),
                max: Value::Const(Constant::Unsigned(MAX)),
            }]))
        }
    }

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

    impl HasTypeSpec for Vec<u8> {
        fn type_spec() -> TypeSpec {
            TypeSpec::Struct(StructBody(alloc::vec![Declaration::VariableOpaque {
                name: ident("unnamed_0"),
                max: Value::Const(Constant::Unsigned(u32::MAX)),
            }]))
        }
    }

    impl Declare for Vec<u8> {
        fn declare(name: Ident) -> Declaration {
            Declaration::VariableOpaque {
                name,
                max: Value::Const(Constant::Unsigned(u32::MAX)),
            }
        }
    }

    impl<T: HasTypeSpec> HasTypeSpec for Vec<T> {
        fn type_spec() -> TypeSpec {
            TypeSpec::Struct(StructBody(alloc::vec![Declaration::VariableArray {
                name: ident("unnamed_0"),
                elt_ty: T::type_spec(),
                max: Value::Const(Constant::Unsigned(u32::MAX)),
            }]))
        }
    }

    impl<T: HasTypeSpec> Declare for Vec<T> {
        fn declare(name: Ident) -> Declaration {
            Declaration::VariableArray {
                name,
                elt_ty: T::type_spec(),
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
            TypeDef::Def(decl) => match decl.name() {
                Some(ty_ident) => declare_named(ty_ident.clone()),
                None => decl,
            },
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

// Tuple implementations - map tuples to inline embedded structs with numbered fields
// Tuples use TypeSpec::Struct (embedded) instead of TypeDef (named type) to avoid
// name conflicts with nested tuples and to embed the struct definition inline in JSON
macro_rules! impl_tuple_types {
    ( $($idx:tt : $t:ident),+ ) => {
        // HasTypeSpec returns an embedded struct, not a named type reference
        impl<$($t),+> HasTypeSpec for ($($t,)+)
        where
            $( $t: Declare, )+
        {
            fn type_spec() -> TypeSpec {
                // Create an embedded struct body
                let body = StructBody(alloc::vec![
                    $(
                        $t::declare(ident(concat!("unnamed_", stringify!($idx)))),
                    )+
                ]);

                // Return embedded struct, not a named type
                TypeSpec::Struct(body)
            }
        }

        // Declare creates a Declaration::Named with the embedded struct TypeSpec
        impl<$($t),+> Declare for ($($t,)+)
        where
            $( $t: Declare, )+
        {
            fn declare(name: Ident) -> Declaration {
                Declaration::Named {
                    name,
                    ty: Self::type_spec(),
                }
            }
        }
    };
}

// Implement for tuples of size 1-16 (matching ReadXDR/WriteXDR implementations)
impl_tuple_types!(0: T0);
impl_tuple_types!(0: T0, 1: T1);
impl_tuple_types!(0: T0, 1: T1, 2: T2);
impl_tuple_types!(0: T0, 1: T1, 2: T2, 3: T3);
impl_tuple_types!(0: T0, 1: T1, 2: T2, 3: T3, 4: T4);
impl_tuple_types!(0: T0, 1: T1, 2: T2, 3: T3, 4: T4, 5: T5);
impl_tuple_types!(0: T0, 1: T1, 2: T2, 3: T3, 4: T4, 5: T5, 6: T6);
impl_tuple_types!(0: T0, 1: T1, 2: T2, 3: T3, 4: T4, 5: T5, 6: T6, 7: T7);
impl_tuple_types!(0: T0, 1: T1, 2: T2, 3: T3, 4: T4, 5: T5, 6: T6, 7: T7, 8: T8);
impl_tuple_types!(0: T0, 1: T1, 2: T2, 3: T3, 4: T4, 5: T5, 6: T6, 7: T7, 8: T8, 9: T9);
impl_tuple_types!(0: T0, 1: T1, 2: T2, 3: T3, 4: T4, 5: T5, 6: T6, 7: T7, 8: T8, 9: T9, 10: T10);
impl_tuple_types!(0: T0, 1: T1, 2: T2, 3: T3, 4: T4, 5: T5, 6: T6, 7: T7, 8: T8, 9: T9, 10: T10, 11: T11);
impl_tuple_types!(0: T0, 1: T1, 2: T2, 3: T3, 4: T4, 5: T5, 6: T6, 7: T7, 8: T8, 9: T9, 10: T10, 11: T11, 12: T12);
impl_tuple_types!(0: T0, 1: T1, 2: T2, 3: T3, 4: T4, 5: T5, 6: T6, 7: T7, 8: T8, 9: T9, 10: T10, 11: T11, 12: T12, 13: T13);
impl_tuple_types!(0: T0, 1: T1, 2: T2, 3: T3, 4: T4, 5: T5, 6: T6, 7: T7, 8: T8, 9: T9, 10: T10, 11: T11, 12: T12, 13: T13, 14: T14);
impl_tuple_types!(0: T0, 1: T1, 2: T2, 3: T3, 4: T4, 5: T5, 6: T6, 7: T7, 8: T8, 9: T9, 10: T10, 11: T11, 12: T12, 13: T13, 14: T14, 15: T15);
