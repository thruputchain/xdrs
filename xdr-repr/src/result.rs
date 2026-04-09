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
                    decl: T::declare(ident("ok")),
                },
                CaseSpec {
                    cases: vec![signed(1)],
                    decl: E::declare(ident("err")),
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
