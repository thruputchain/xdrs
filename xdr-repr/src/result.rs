use crate::xdr::*;
use crate::ty::*;
use alloc::string::*;

// Geoff note: I don't know what the optimial design for doing this is.
// What I've done here is to move the generic handling from the xdrs crate to the spec generation
// in sdk.  This is a bit of a hack, but it seems easy enough to do the generics handling
// at the top layer (so we can list out the types that need to be generated),
// instead of here.

impl <T, E> HasTypeSpec for Result<T, E>
    where T: HasTypeSpec
{
    fn type_spec() -> TypeSpec {
        T::type_spec()
    }
}

// TODO(geoff): is there a better way to handle void?
impl<E> HasTypeSpec for Result<(), E> {
    fn type_spec() -> TypeSpec {
        TypeSpec::Ident(Ident("void".to_string()))
    }
}


// As XDR has no generics, we make do here by mashing
// names together to get, e.g., ExpectedInt.  But you'll
// have to arrange for these names to be defined.
/*
impl<T, E> HasTypeSpec for Result<T, E>
    where T: HasTypeName
{
    fn type_spec() -> TypeSpec {
        TypeSpec::Ident(Ident("Expected".to_string() + &T::name()))
    }
} 

trait HasTypeName {
    fn name() -> String;
}

macro_rules! impl_has_type_name {
    { $( $ty:ty => $name:expr ),* $(,)? } =>
    {$(
        impl HasTypeName for $ty {
            fn name() -> String {
                $name.to_string()
            }
        }
    )*};
}

impl_has_type_name! {
    () => "Void",
    i32 => "Int",
    u32 => "Unsigned",
    i64 => "Hyper",
    u64 => "UnsignedHyper",
    bool => "Bool",
    u128 => "U128",
    i128 => "I128",
}

// If you've defined type Foo, this gives it a "name" so that we can have a typespec (the
// identifier "ExpectedFoo") for type Result<Foo, _>
impl<T> HasTypeName for T
    where T: HasTypeDef
{
    fn name() -> String {
        type_def_ident(T::type_def()).0
    }
} */
