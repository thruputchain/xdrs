//! Support for representing `Result` types in XDR.
//!
//! `Result<T, E>` is modeled as an anonymous embedded union so the generated
//! XDR representation matches the runtime encoding:
//!
//! - `which = 0` carries the `ok` payload
//! - `which = 1` carries the `err` payload
//!
//! This keeps `Result<T, E>` inline in parent declarations instead of inventing
//! a synthetic named type.

use crate::declare_int;
use crate::ident;
use crate::signed;
use crate::ty::*;
use crate::xdr::*;
use alloc::boxed::Box;
use alloc::vec;

fn rename_declaration(decl: Declaration, name: Ident) -> Declaration {
    match decl {
        Declaration::Named { ty, .. } => Declaration::Named { name, ty },
        Declaration::FixedOpaque { size, .. } => Declaration::FixedOpaque { name, size },
        Declaration::VariableOpaque { max, .. } => Declaration::VariableOpaque { name, max },
        Declaration::String { max, .. } => Declaration::String { name, max },
        Declaration::FixedArray { elt_ty, len, .. } => {
            Declaration::FixedArray { name, elt_ty, len }
        }
        Declaration::VariableArray { elt_ty, max, .. } => {
            Declaration::VariableArray { name, elt_ty, max }
        }
        Declaration::Optional { opt_ty, .. } => Declaration::Optional { name, opt_ty },
        Declaration::Void => Declaration::Void,
    }
}

impl<T, E> HasTypeSpec for Result<T, E>
where
    T: Declare,
    E: Declare,
{
    fn type_spec() -> TypeSpec {
        TypeSpec::Union(Box::new(UnionBody {
            discriminant_decl: declare_int("which"),
            case_specs: vec![
                CaseSpec {
                    cases: vec![signed(0)],
                    decl: rename_declaration(T::declare(ident("ok")), ident("ok")),
                },
                CaseSpec {
                    cases: vec![signed(1)],
                    decl: rename_declaration(E::declare(ident("err")), ident("err")),
                },
            ],
            default_decl: None,
        }))
    }
}

impl<T, E> Declare for Result<T, E>
where
    T: Declare,
    E: Declare,
{
    fn declare(name: Ident) -> Declaration {
        Declaration::Named {
            name,
            ty: Self::type_spec(),
        }
    }
}

impl<T, E> HasTypeSpec for &Result<T, E>
where
    T: Declare,
    E: Declare,
{
    fn type_spec() -> TypeSpec {
        Result::<T, E>::type_spec()
    }
}

impl<T, E> Declare for &Result<T, E>
where
    T: Declare,
    E: Declare,
{
    fn declare(name: Ident) -> Declaration {
        Result::<T, E>::declare(name)
    }
}

impl<T, E> HasTypeSpec for &mut Result<T, E>
where
    T: Declare,
    E: Declare,
{
    fn type_spec() -> TypeSpec {
        Result::<T, E>::type_spec()
    }
}

impl<T, E> Declare for &mut Result<T, E>
where
    T: Declare,
    E: Declare,
{
    fn declare(name: Ident) -> Declaration {
        Result::<T, E>::declare(name)
    }
}

/// Trait for extracting the error type from a `Result` type.
///
/// This trait allows querying the error type of a `Result<T, E>` for
/// validation purposes in XDR specification generation.
pub trait GetErrorType {
    /// Returns the XDR type specification of the error type, if any.
    ///
    /// For `Result<T, E>` types where `E: HasTypeSpec`, this returns `Some(E::type_spec())`.
    /// For other types, this returns `None`.
    fn get_error_type() -> Option<TypeSpec> {
        None
    }
}

impl<T, E> GetErrorType for Result<T, E>
where
    E: HasTypeSpec,
{
    fn get_error_type() -> Option<TypeSpec> {
        Some(E::type_spec())
    }
}
