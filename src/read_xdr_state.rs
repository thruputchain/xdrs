use crate::serialize::reader::*;
use crate::serialize::reader_path::*;
use crate::read_xdr::*;
use crate::valid_u32::*;

use crate::mu;
use core::mem::MaybeUninit as MU;

pub trait ReadXDRPath<'r, P>: Sized {
    fn read_xdr_path<R: ReaderPath<'r, P>>(r: &mut R) -> Result<Self>;
}

pub fn read_xdr_path<'r, H, R, T>(r: &mut R) -> Result<T>
where
    H: PathHandler,
    R: ReaderPath<'r, H>,
    T: ReadXDRPath<'r, H::Path>,
{
    T::read_xdr_path(r)
}

/// Types that can be constructed by reading XDR data
/// of any lifetime
pub trait ReadXDRPathOwned<P>: for<'r> ReadXDRPath<'r, P> {}

impl<T, P> ReadXDRPathOwned<P> for T where T: for<'r> ReadXDRPath<'r, P> {}

macro_rules! delegate_impl {
    { $ty:ty } => {

        impl<'r ,P> ReadXDRPath<'r, P> for $ty {
            fn read_xdr_path<R: ReaderPath<'r, P>>(r: &mut R) -> Result<$ty> {
                read_xdr(r)
            }
        }

    };
}

macro_rules! delegate_impls {
    { $( $ty:ty ),* $(,)? } => { $(
        delegate_impl! { $ty }
    )* };
}

#[allow(unused)]
macro_rules! delegate_const_param_impl {
    { <const $param:ident: $const_ty:ty> $ty:ty } => {

        impl<'r, P, const $param: $const_ty> ReadXDRPath<'r, P> for $ty {
            fn read_xdr_path<R: ReaderPath<'r, P>>(r: &mut R) -> Result<$ty> {
                read_xdr(r)
            }
        }

    };
}

delegate_impls! {
    bool,
    i64, i32, i16,
    u64, u32, u16,
    (),
    &'r [u8],
}

/* XXX
delegate_const_param_impl! {
    <const N: usize> [u8; N]
}
*/

#[cfg(all(feature = "extras"))]
delegate_impls! {
    i128, u128
}

#[cfg(feature = "alloc")]
delegate_impls! {
    alloc::string::String,
}

impl<'r, P, T> ReadXDRPath<'r, P> for (T,)
where
    T: ReadXDRPath<'r, P>,
{
    fn read_xdr_path<R: ReaderPath<'r, P>>(r: &mut R) -> Result<Self> {
        Ok((read_xdr_path(r)?,))
    }
}

macro_rules! read_tuple {
    { $( $t:ident ),+ } => {

        impl<'r, P, $($t),+> ReadXDRPath<'r, P> for ($($t),+)
        where
            $( $t: ReadXDRPath<'r, P> ),+
        {
            fn read_xdr_path<R: ReaderPath<'r, P>>(r: &mut R) -> Result<Self> {
                r.sequence_begin();
                let val = (
                    $( {
                        let v: $t = read_xdr_path(r)?;
                        r.sequence_next();
                        v
                    } ),+
                );
                r.sequence_end();

                Ok(val)
            }
        }

    };
}

read_tuple!(T1, T2);
read_tuple!(T1, T2, T3);
read_tuple!(T1, T2, T3, T4);
read_tuple!(T1, T2, T3, T4, T5);
read_tuple!(T1, T2, T3, T4, T5, T6);

impl<'r, P, T, const N: usize> ReadXDRPath<'r, P> for [T; N]
where
    T: ReadXDRPath<'r, P>
{
    fn read_xdr_path<R: ReaderPath<'r, P>>(r: &mut R) -> Result<Self> {
        let () = ValidU32::<N>::OK;

        let mut arr: [MU<T>; N] = [const { MU::uninit() }; N];

        r.sequence_begin();

        // TODO: catch errors to avoid memory leaks
        for elt in arr.iter_mut() {
            elt.write(read_xdr_path(r)?);
            r.sequence_next();
        }

        r.sequence_end();

        // SAFETY: all elements have been initialized
        let arr = unsafe { mu::array_assume_init(arr) };

        Ok(arr)
    }
}

impl<'r, P, T> ReadXDRPath<'r, P> for Option<T>
where
    T: ReadXDRPath<'r, P>
{
    fn read_xdr_path<R: ReaderPath<'r, P>>(r: &mut R) -> Result<Self> {
        let is_present = r.read_bool()?;
        if is_present {
            Ok(Some(read_xdr_path(r)?))
        } else {
            Ok(None)
        }
    }
}

#[cfg(all(feature = "xdr_common", feature = "alloc"))]
impl<'r, P, T> ReadXDRPath<'r, P> for xdr_common::Box<T>
where
    T: ReadXDRPath<'r, P>
{
    fn read_xdr_path<R: ReaderPath<'r, P>>(r: &mut R) -> Result<Self> {
        Ok(Self::new(read_xdr_path(r)?))
    }
}

impl<'r, P, T, E> ReadXDRPath<'r, P> for core::result::Result<T, E>
where
    T: ReadXDRPath<'r, P>,
    E: ReadXDRPath<'r, P>,
{
    fn read_xdr_path<R: ReaderPath<'r, P>>(r: &mut R) -> Result<Self> {
        let d = r.read_i32()?;

        match d {
            0 => Ok(Result::Ok(read_xdr_path(r)?)),
            1 => Ok(Result::Err(read_xdr_path(r)?)),
            _ => Err(Error::BadDiscriminator(d)),
        }
    }
}

impl<'r, P, T> ReadXDRPath<'r, P> for core::marker::PhantomData<T> {
    fn read_xdr_path<R: ReaderPath<'r, P>>(_r: &mut R) -> Result<Self> {
        Ok(core::marker::PhantomData)
    }
}

#[cfg(feature = "alloc")]
const _: () = {
    use alloc::vec::Vec;

    delegate_impls! { Vec<u8> }

    impl<'r, P, T> ReadXDRPath<'r, P> for Vec<T>
    where
        T: ReadXDRPath<'r, P>
    {
        fn read_xdr_path<R: ReaderPath<'r, P>>(r: &mut R) -> Result<Self> {
            let len = r.read_u32()? as usize;

            let mut vec: Vec<T> = Vec::with_capacity(len);
            let slice = vec.spare_capacity_mut();

            r.sequence_begin();

            // TODO: catch errors to avoid memory leaks
            for elt in slice.iter_mut() {
                elt.write(read_xdr_path(r)?);
                r.sequence_next();
            }

            r.sequence_end();

            // SAFETY: all elts up to `len` have been initialized
            unsafe { vec.set_len(len) }

            Ok(vec)
        }
    }
};

#[cfg(all(feature = "xdr_common", feature = "alloc"))]
const _: () = {
    use xdr_common::*;
    use crate::serialize::reader::{Error, Result};

    delegate_const_param_impl! {
        <const MAX: u32> BytesM<MAX>
    }

    impl<'r, P, const MAX: u32, T> ReadXDRPath<'r, P> for VecM<T, MAX>
    where
        T: ReadXDRPath<'r, P>
    {
        fn read_xdr_path<R: ReaderPath<'r, P>>(r: &mut R) -> Result<Self> {
            let vec: Vec<T> = read_xdr_path(r)?;
            if vec.len() > MAX as usize {
                return Err(Error::MaxLengthExceeded);
            }
            Ok(Self(vec))
        }
    }
};
