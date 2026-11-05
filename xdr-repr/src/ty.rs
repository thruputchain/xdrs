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

    /// Returns the XDR identifier for this named type.
    fn type_ident() -> Ident {
        type_def_ident(Self::type_def())
    }

    /// Creates a declaration that references this named type.
    fn named_declaration(name: Ident) -> Declaration {
        named_declaration_from_type_def(name, Self::type_def())
    }
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
        TypeSpec::Ident(T::type_ident())
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

fn named_declaration_from_type_def(name: Ident, type_def: TypeDef) -> Declaration {
    let declare_named = |ty_ident| Declaration::Named {
        name,
        ty: TypeSpec::Ident(ty_ident),
    };

    match type_def {
        TypeDef::Def(decl) => match decl.name() {
            Some(ty_ident) => declare_named(ty_ident.clone()),
            None => decl,
        },
        TypeDef::Struct(ty_ident, _) => declare_named(ty_ident),
        TypeDef::Enum(ty_ident, _) => declare_named(ty_ident),
        TypeDef::Union(ty_ident, _) => declare_named(ty_ident),
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
    f32 => TypeSpec::Float,
    f64 => TypeSpec::Double,
}

// 128-bit integer types using the alternate XDR encoding. `i128` and `u128`
// are represented as named types referring to double-u64 structures defined
// outside this package.
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
    f32 => TypeSpec::Float,
    f64 => TypeSpec::Double,
}

// These refer to double-u64 structures defined outside this package.
impl_declare_named! {
    i128 => <(i64, u64)>::type_spec(),
    u128 => <(u64, u64)>::type_spec(),
}

macro_rules! impl_refs {
    ($($ty:ty),* $(,)?) => {$(
        impl HasTypeSpec for &$ty {
            fn type_spec() -> TypeSpec {
                <$ty>::type_spec()
            }
        }
        impl HasTypeSpec for &mut $ty {
            fn type_spec() -> TypeSpec {
                <$ty>::type_spec()
            }
        }
        impl Declare for &$ty {
            fn declare(name: Ident) -> Declaration {
                <$ty>::declare(name)
            }
        }
        impl Declare for &mut $ty {
            fn declare(name: Ident) -> Declaration {
                <$ty>::declare(name)
            }
        }
    )*};
}

impl_refs! {
    i32, u32, i64, u64, bool, f32, f64,
}

impl_refs! {
    i128, u128,
}

impl<T: HasTypeSpec> HasTypeSpec for Option<T> {
    fn type_spec() -> TypeSpec {
        // Options aren't types in XDR, so we represent them as embedded structs
        // with a single optional field named "unnamed_0"
        TypeSpec::Struct(StructBody::new(alloc::vec![Declaration::Optional {
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

        fn type_ident() -> Ident {
            T::type_ident()
        }

        fn named_declaration(name: Ident) -> Declaration {
            T::named_declaration(name)
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
        TypeSpec::Struct(StructBody::new(alloc::vec![Declaration::FixedOpaque {
            name: ident("unnamed_0"),
            size: Value::Const((N as u32).into()),
        }]))
    }
}

impl<const N: usize> Declare for &[u8; N] {
    fn declare(name: Ident) -> Declaration {
        <[u8; N]>::declare(name)
    }
}

impl<const N: usize> HasTypeSpec for &[u8; N] {
    fn type_spec() -> TypeSpec {
        <[u8; N]>::type_spec()
    }
}

impl<const N: usize> Declare for &mut [u8; N] {
    fn declare(name: Ident) -> Declaration {
        <[u8; N]>::declare(name)
    }
}

impl<const N: usize> HasTypeSpec for &mut [u8; N] {
    fn type_spec() -> TypeSpec {
        <[u8; N]>::type_spec()
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
        TypeSpec::Struct(StructBody::new(alloc::vec![Declaration::FixedArray {
            name: ident("unnamed_0"),
            elt_ty: T::type_spec(),
            len: Value::Const((N as u32).into()),
        }]))
    }
}

impl<const N: usize, T: HasTypeSpec> HasTypeSpec for &[T; N] {
    fn type_spec() -> TypeSpec {
        <[T; N]>::type_spec()
    }
}

impl<const N: usize, T: HasTypeSpec> Declare for &[T; N] {
    fn declare(name: Ident) -> Declaration {
        <[T; N]>::declare(name)
    }
}

impl<const N: usize, T: HasTypeSpec> HasTypeSpec for &mut [T; N] {
    fn type_spec() -> TypeSpec {
        <[T; N]>::type_spec()
    }
}

impl<const N: usize, T: HasTypeSpec> Declare for &mut [T; N] {
    fn declare(name: Ident) -> Declaration {
        <[T; N]>::declare(name)
    }
}

impl HasTypeSpec for &[u8] {
    fn type_spec() -> TypeSpec {
        TypeSpec::Struct(StructBody::new(alloc::vec![Declaration::VariableOpaque {
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

impl HasTypeSpec for &mut [u8] {
    fn type_spec() -> TypeSpec {
        <&[u8]>::type_spec()
    }
}

impl Declare for &mut [u8] {
    fn declare(name: Ident) -> Declaration {
        <&[u8]>::declare(name)
    }
}

impl HasTypeSpec for &str {
    fn type_spec() -> TypeSpec {
        TypeSpec::Struct(StructBody::new(alloc::vec![<&str as Declare>::declare(
            ident("unnamed_0")
        ),]))
    }
}

impl Declare for &str {
    fn declare(name: Ident) -> Declaration {
        Declaration::String {
            name,
            max: Value::Const(Constant::Unsigned(u32::MAX)),
        }
    }
}

impl HasTypeSpec for &mut str {
    fn type_spec() -> TypeSpec {
        <&str>::type_spec()
    }
}

impl Declare for &mut str {
    fn declare(name: Ident) -> Declaration {
        <&str>::declare(name)
    }
}

impl<T: HasTypeSpec> HasTypeSpec for &[T] {
    fn type_spec() -> TypeSpec {
        TypeSpec::Struct(StructBody::new(alloc::vec![<&[T] as Declare>::declare(
            ident("unnamed_0"),
        )]))
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

impl<T: HasTypeSpec> HasTypeSpec for &mut [T] {
    fn type_spec() -> TypeSpec {
        <&[T]>::type_spec()
    }
}

impl<T: HasTypeSpec> Declare for &mut [T] {
    fn declare(name: Ident) -> Declaration {
        <&[T]>::declare(name)
    }
}

const _: () = {
    use xdr_common::{BytesM, StaticIfNoAlloc, StringM, VecM};

    impl<T, const MAX: u32> HasTypeSpec for VecM<T, MAX>
    where
        T: HasTypeSpec + StaticIfNoAlloc,
    {
        fn type_spec() -> TypeSpec {
            TypeSpec::Struct(StructBody::new(alloc::vec![Declaration::VariableArray {
                name: ident("unnamed_0"),
                elt_ty: T::type_spec(),
                max: Value::Const(Constant::Unsigned(MAX)),
            }]))
        }
    }

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
            TypeSpec::Struct(StructBody::new(alloc::vec![Declaration::VariableOpaque {
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

    impl<const MAX: u32> HasTypeSpec for StringM<MAX> {
        fn type_spec() -> TypeSpec {
            TypeSpec::Struct(StructBody::new(alloc::vec![Declaration::String {
                name: ident("unnamed_0"),
                max: Value::Const(Constant::Unsigned(MAX)),
            }]))
        }
    }

    impl<const MAX: u32> HasTypeSpec for &BytesM<MAX> {
        fn type_spec() -> TypeSpec {
            BytesM::<MAX>::type_spec()
        }
    }

    impl<const MAX: u32> Declare for &BytesM<MAX> {
        fn declare(name: Ident) -> Declaration {
            BytesM::<MAX>::declare(name)
        }
    }

    impl<const MAX: u32> HasTypeSpec for &mut BytesM<MAX> {
        fn type_spec() -> TypeSpec {
            BytesM::<MAX>::type_spec()
        }
    }

    impl<const MAX: u32> Declare for &mut BytesM<MAX> {
        fn declare(name: Ident) -> Declaration {
            BytesM::<MAX>::declare(name)
        }
    }

    impl<const MAX: u32> HasTypeSpec for &StringM<MAX> {
        fn type_spec() -> TypeSpec {
            StringM::<MAX>::type_spec()
        }
    }

    impl<const MAX: u32> Declare for &StringM<MAX> {
        fn declare(name: Ident) -> Declaration {
            StringM::<MAX>::declare(name)
        }
    }

    impl<const MAX: u32> HasTypeSpec for &mut StringM<MAX> {
        fn type_spec() -> TypeSpec {
            StringM::<MAX>::type_spec()
        }
    }

    impl<const MAX: u32> Declare for &mut StringM<MAX> {
        fn declare(name: Ident) -> Declaration {
            StringM::<MAX>::declare(name)
        }
    }

    impl<const MAX: u32> HasTypeSpec for &VecM<u8, MAX> {
        fn type_spec() -> TypeSpec {
            VecM::<u8, MAX>::type_spec()
        }
    }

    impl<const MAX: u32> Declare for &VecM<u8, MAX> {
        fn declare(name: Ident) -> Declaration {
            VecM::<u8, MAX>::declare(name)
        }
    }

    impl<const MAX: u32> HasTypeSpec for &mut VecM<u8, MAX> {
        fn type_spec() -> TypeSpec {
            VecM::<u8, MAX>::type_spec()
        }
    }

    impl<const MAX: u32> Declare for &mut VecM<u8, MAX> {
        fn declare(name: Ident) -> Declaration {
            VecM::<u8, MAX>::declare(name)
        }
    }

    impl<T, const MAX: u32> HasTypeSpec for &VecM<T, MAX>
    where
        T: HasTypeSpec + StaticIfNoAlloc,
    {
        fn type_spec() -> TypeSpec {
            VecM::<T, MAX>::type_spec()
        }
    }

    impl<T, const MAX: u32> Declare for &VecM<T, MAX>
    where
        T: HasTypeSpec + StaticIfNoAlloc,
    {
        fn declare(name: Ident) -> Declaration {
            VecM::<T, MAX>::declare(name)
        }
    }

    impl<T, const MAX: u32> HasTypeSpec for &mut VecM<T, MAX>
    where
        T: HasTypeSpec + StaticIfNoAlloc,
    {
        fn type_spec() -> TypeSpec {
            VecM::<T, MAX>::type_spec()
        }
    }

    impl<T, const MAX: u32> Declare for &mut VecM<T, MAX>
    where
        T: HasTypeSpec + StaticIfNoAlloc,
    {
        fn declare(name: Ident) -> Declaration {
            VecM::<T, MAX>::declare(name)
        }
    }
};

const _: () = {
    use alloc::boxed::Box;
    use xdr_common::{BytesM, StaticIfNoAlloc, StringM, VecM};

    impl<const MAX: u32> Declare for Box<BytesM<MAX>> {
        fn declare(name: Ident) -> Declaration {
            BytesM::<MAX>::declare(name)
        }
    }

    impl<const MAX: u32> Declare for Box<StringM<MAX>> {
        fn declare(name: Ident) -> Declaration {
            StringM::<MAX>::declare(name)
        }
    }

    impl<T, const MAX: u32> Declare for Box<VecM<T, MAX>>
    where
        T: HasTypeSpec + StaticIfNoAlloc,
    {
        fn declare(name: Ident) -> Declaration {
            VecM::<T, MAX>::declare(name)
        }
    }

    impl<const MAX: u32> Declare for Box<VecM<u8, MAX>> {
        fn declare(name: Ident) -> Declaration {
            BytesM::<MAX>::declare(name)
        }
    }

    impl<const MAX: u32> HasTypeSpec for Box<BytesM<MAX>> {
        fn type_spec() -> TypeSpec {
            BytesM::<MAX>::type_spec()
        }
    }

    impl<const MAX: u32> HasTypeSpec for Box<StringM<MAX>> {
        fn type_spec() -> TypeSpec {
            StringM::<MAX>::type_spec()
        }
    }

    impl<T, const MAX: u32> HasTypeSpec for Box<VecM<T, MAX>>
    where
        T: HasTypeSpec + StaticIfNoAlloc,
    {
        fn type_spec() -> TypeSpec {
            VecM::<T, MAX>::type_spec()
        }
    }

    impl<const MAX: u32> HasTypeSpec for Box<VecM<u8, MAX>> {
        fn type_spec() -> TypeSpec {
            BytesM::<MAX>::type_spec()
        }
    }

    impl<T: HasTypeSpec> Declare for Box<Option<T>> {
        fn declare(name: Ident) -> Declaration {
            <Option<T>>::declare(name)
        }
    }

    impl<T: HasTypeSpec> HasTypeSpec for Box<Option<T>> {
        fn type_spec() -> TypeSpec {
            <Option<T>>::type_spec()
        }
    }
};

#[cfg(feature = "heapless")]
const _: () = {
    use heapless::{String, Vec};

    fn heapless_max_value<const N: usize>() -> Value {
        const { assert!(N <= u32::MAX as usize, "N exceeds u32::MAX") }
        Value::Const(Constant::Unsigned(N as u32))
    }

    impl<const N: usize> HasTypeSpec for Vec<u8, N> {
        fn type_spec() -> TypeSpec {
            TypeSpec::Struct(StructBody::new(alloc::vec![Declaration::VariableOpaque {
                name: ident("unnamed_0"),
                max: heapless_max_value::<N>(),
            }]))
        }
    }

    impl<const N: usize> Declare for Vec<u8, N> {
        fn declare(name: Ident) -> Declaration {
            Declaration::VariableOpaque {
                name,
                max: heapless_max_value::<N>(),
            }
        }
    }

    impl<const N: usize> HasTypeSpec for &Vec<u8, N> {
        fn type_spec() -> TypeSpec {
            Vec::<u8, N>::type_spec()
        }
    }

    impl<const N: usize> Declare for &Vec<u8, N> {
        fn declare(name: Ident) -> Declaration {
            Vec::<u8, N>::declare(name)
        }
    }

    impl<const N: usize> HasTypeSpec for &mut Vec<u8, N> {
        fn type_spec() -> TypeSpec {
            Vec::<u8, N>::type_spec()
        }
    }

    impl<const N: usize> Declare for &mut Vec<u8, N> {
        fn declare(name: Ident) -> Declaration {
            Vec::<u8, N>::declare(name)
        }
    }

    impl<T, const N: usize> HasTypeSpec for Vec<T, N>
    where
        T: HasTypeSpec,
    {
        fn type_spec() -> TypeSpec {
            TypeSpec::Struct(StructBody::new(alloc::vec![Declaration::VariableArray {
                name: ident("unnamed_0"),
                elt_ty: T::type_spec(),
                max: heapless_max_value::<N>(),
            }]))
        }
    }

    impl<T, const N: usize> Declare for Vec<T, N>
    where
        T: HasTypeSpec,
    {
        fn declare(name: Ident) -> Declaration {
            Declaration::VariableArray {
                name,
                elt_ty: T::type_spec(),
                max: heapless_max_value::<N>(),
            }
        }
    }

    impl<T, const N: usize> HasTypeSpec for &Vec<T, N>
    where
        T: HasTypeSpec,
    {
        fn type_spec() -> TypeSpec {
            Vec::<T, N>::type_spec()
        }
    }

    impl<T, const N: usize> Declare for &Vec<T, N>
    where
        T: HasTypeSpec,
    {
        fn declare(name: Ident) -> Declaration {
            Vec::<T, N>::declare(name)
        }
    }

    impl<T, const N: usize> HasTypeSpec for &mut Vec<T, N>
    where
        T: HasTypeSpec,
    {
        fn type_spec() -> TypeSpec {
            Vec::<T, N>::type_spec()
        }
    }

    impl<T, const N: usize> Declare for &mut Vec<T, N>
    where
        T: HasTypeSpec,
    {
        fn declare(name: Ident) -> Declaration {
            Vec::<T, N>::declare(name)
        }
    }

    impl<const N: usize> HasTypeSpec for String<N> {
        fn type_spec() -> TypeSpec {
            TypeSpec::Struct(StructBody::new(alloc::vec![Declaration::String {
                name: ident("unnamed_0"),
                max: heapless_max_value::<N>(),
            }]))
        }
    }

    impl<const N: usize> Declare for String<N> {
        fn declare(name: Ident) -> Declaration {
            Declaration::String {
                name,
                max: heapless_max_value::<N>(),
            }
        }
    }

    impl<const N: usize> HasTypeSpec for &String<N> {
        fn type_spec() -> TypeSpec {
            String::<N>::type_spec()
        }
    }

    impl<const N: usize> Declare for &String<N> {
        fn declare(name: Ident) -> Declaration {
            String::<N>::declare(name)
        }
    }

    impl<const N: usize> HasTypeSpec for &mut String<N> {
        fn type_spec() -> TypeSpec {
            String::<N>::type_spec()
        }
    }

    impl<const N: usize> Declare for &mut String<N> {
        fn declare(name: Ident) -> Declaration {
            String::<N>::declare(name)
        }
    }
};

const _: () = {
    use alloc::vec::Vec;

    impl HasTypeSpec for Vec<u8> {
        fn type_spec() -> TypeSpec {
            TypeSpec::Struct(StructBody::new(alloc::vec![Declaration::VariableOpaque {
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
            TypeSpec::Struct(StructBody::new(alloc::vec![Declaration::VariableArray {
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

    impl HasTypeSpec for alloc::string::String {
        fn type_spec() -> TypeSpec {
            TypeSpec::Struct(StructBody::new(alloc::vec![
                <alloc::string::String as Declare>::declare(ident("unnamed_0")),
            ]))
        }
    }

    impl Declare for alloc::string::String {
        fn declare(name: Ident) -> Declaration {
            Declaration::String {
                name,
                max: Value::Const(Constant::Unsigned(u32::MAX)),
            }
        }
    }

    impl HasTypeSpec for &Vec<u8> {
        fn type_spec() -> TypeSpec {
            Vec::<u8>::type_spec()
        }
    }

    impl Declare for &Vec<u8> {
        fn declare(name: Ident) -> Declaration {
            Vec::<u8>::declare(name)
        }
    }

    impl HasTypeSpec for &mut Vec<u8> {
        fn type_spec() -> TypeSpec {
            Vec::<u8>::type_spec()
        }
    }

    impl Declare for &mut Vec<u8> {
        fn declare(name: Ident) -> Declaration {
            Vec::<u8>::declare(name)
        }
    }

    impl<T: HasTypeSpec> HasTypeSpec for &Vec<T> {
        fn type_spec() -> TypeSpec {
            Vec::<T>::type_spec()
        }
    }

    impl<T: HasTypeSpec> Declare for &Vec<T> {
        fn declare(name: Ident) -> Declaration {
            Vec::<T>::declare(name)
        }
    }

    impl<T: HasTypeSpec> HasTypeSpec for &mut Vec<T> {
        fn type_spec() -> TypeSpec {
            Vec::<T>::type_spec()
        }
    }

    impl<T: HasTypeSpec> Declare for &mut Vec<T> {
        fn declare(name: Ident) -> Declaration {
            Vec::<T>::declare(name)
        }
    }

    impl HasTypeSpec for &alloc::string::String {
        fn type_spec() -> TypeSpec {
            alloc::string::String::type_spec()
        }
    }

    impl Declare for &alloc::string::String {
        fn declare(name: Ident) -> Declaration {
            alloc::string::String::declare(name)
        }
    }

    impl HasTypeSpec for &mut alloc::string::String {
        fn type_spec() -> TypeSpec {
            alloc::string::String::type_spec()
        }
    }

    impl Declare for &mut alloc::string::String {
        fn declare(name: Ident) -> Declaration {
            alloc::string::String::declare(name)
        }
    }
};

/// Unit declares as `void`, which is valid as a standalone struct
/// field or union arm but produces invalid XDR-IDL when nested inside
/// container types (`Option<()>`, `Vec<()>`, `VecM<()>`, `[(); N]`,
/// etc.).  The runtime codecs handle these compositions correctly,
/// but the schema output is not meaningful.  Avoid `()` inside
/// generic wrappers in types that require schema generation.
impl Declare for () {
    fn declare(_name: Ident) -> Declaration {
        Declaration::Void
    }
}

impl<T> Declare for core::marker::PhantomData<T> {
    fn declare(_name: Ident) -> Declaration {
        Declaration::Void
    }
}

impl<T> Declare for T
where
    T: HasTypeDef,
{
    fn declare(name: Ident) -> Declaration {
        T::named_declaration(name)
    }
}

// TODO: feature-gate this, as some users may not want it?
impl<T: HasTypeDef> HasTypeDef for &T {
    fn type_def() -> TypeDef {
        T::type_def()
    }

    fn type_ident() -> Ident {
        T::type_ident()
    }

    fn named_declaration(name: Ident) -> Declaration {
        T::named_declaration(name)
    }
}

impl<T: HasTypeDef> HasTypeDef for &mut T {
    fn type_def() -> TypeDef {
        T::type_def()
    }

    fn type_ident() -> Ident {
        T::type_ident()
    }

    fn named_declaration(name: Ident) -> Declaration {
        T::named_declaration(name)
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
                let body = StructBody::new(alloc::vec![
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
