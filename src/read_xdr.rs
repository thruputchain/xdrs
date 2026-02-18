//! Reading XDR data into Rust types.
//!
//! This module provides the [`ReadXDR`] trait and implementations for
//! converting [XDR][RFC-4506]-encoded data (as defined in RFC 4506)
//! into Rust values.
//!
//! [RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506

use crate::mu;
use crate::serialize::reader::*;
use crate::valid_u32::*;
use core::convert::Infallible;
use core::mem::MaybeUninit as MU;

/// Reads an XDR value of type `T` from the reader `r`.
///
/// This is a convenience function that calls [`ReadXDR::read_xdr`] on the type.
///
/// # Example
///
/// ```rust
/// use xdrs::cxs;
/// // Valid CXS-encoded u32 value of 0 (just one byte)
/// let data = [0u8];
/// let _value: u32 = cxs::decode(&data)?;
/// # Ok::<(), xdrs::ReadError>(())
/// ```
pub fn read_xdr<'r, R, T>(r: &mut R) -> ReadResult<T>
where
    R: Reader<'r>,
    T: ReadXDR<'r>,
{
    <T as ReadXDR>::read_xdr(r)
}

/// Types that can be constructed by reading XDR-encoded data, possibly
/// sharing data from the input.
///
/// The lifetime parameter `'r` represents the lifetime of data borrowed
/// from the reader.
///
/// # Example
///
/// ```rust
/// use xdrs::{ReadXDR, cxs};
///
/// let data = [0u8]; // valid CXS encoding for the unsigned integer 0
/// let _value: u32 = cxs::decode(&data)?;
/// # Ok::<(), xdrs::ReadError>(())
/// ```
pub trait ReadXDR<'r>: Sized {
    /// Reads an XDR value from the reader.
    ///
    /// This method should read the appropriate amount of data from the reader
    /// and construct a value of type `Self`.
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self>;
}

/// Types that can be constructed by reading XDR data
/// of any lifetime.
///
/// This trait is automatically implemented for any type that implements
/// [`ReadXDR`] for all lifetimes. It's useful for types that don't borrow
/// data from the reader.
pub trait ReadXDROwned: for<'r> ReadXDR<'r> {}

impl<T> ReadXDROwned for T where T: for<'r> ReadXDR<'r> {}

macro_rules! construct_int_impls {
    { $( $read_method:ident for $int_ty:ty ),* $(,)? } => { $(

        impl<'r> ReadXDR<'r> for $int_ty {
            fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<$int_ty> {
                r.$read_method()
            }
        }

    )* };
}

construct_int_impls! {
    read_i64 for i64,
    read_i32 for i32,
    read_i16 for i16,

    read_u64 for u64,
    read_u32 for u32,
    read_u16 for u16,

    read_bool for bool,
}

impl<'r, T, const N: usize> ReadXDR<'r> for [T; N]
where
    T: ReadXDR<'r>,
{
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        let () = ValidU32::<N>::OK;

        let mut arr: [MU<T>; N] = [const { MU::uninit() }; N];

        // Initialize elements one by one, dropping already-initialized elements on error
        for (i, elt) in arr.iter_mut().enumerate() {
            match read_xdr(r) {
                Ok(val) => {
                    elt.write(val);
                }
                Err(e) => {
                    // Drop all initialized elements to avoid memory leak
                    arr.iter_mut().take(i).for_each(|elt| unsafe {
                        elt.assume_init_drop();
                    });
                    return Err(e);
                }
            }
        }

        // SAFETY: all elements have been initialized
        let arr = unsafe { mu::array_assume_init(arr) };

        Ok(arr)
    }
}

impl<'r> ReadXDR<'r> for &'r [u8] {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        r.read_opaque_variable(u32::MAX)
    }
}

impl<'r, const N: usize> ReadXDR<'r> for &'r [u8; N] {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        let () = ValidU32::<N>::OK;
        let len = N as u32;

        // PANIC: We rely on `Reader::read_opaque_fixed` to return a slice of
        // the requested size
        let bytes: &[u8; N] = r.read_opaque_fixed(len)?.try_into().unwrap();
        Ok(bytes)
    }
}

impl<'r, const N: usize> ReadXDR<'r> for [u8; N] {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        let () = ValidU32::<N>::OK;

        let mut arr: [MU<u8>; N] = [MU::uninit(); N];

        r.read_opaque_fixed_into(&mut arr, N as u32)?;

        // SAFETY: We rely on `read_opaque_fixed_into` to init all elements
        let arr = unsafe { mu::array_assume_init(arr) };

        Ok(arr)
    }
}

impl<'r> ReadXDR<'r> for &'r str {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        let bytes = r.read_opaque_variable(u32::MAX)?;
        str::from_utf8(bytes).map_err(|_| ReadError::BadUTF8)
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl<'r> ReadXDR<'r> for alloc::string::String {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        let bytes = r.read_opaque_variable(u32::MAX)?;
        let v = alloc::vec::Vec::from(bytes);
        let s = alloc::string::String::from_utf8(v).map_err(|_| ReadError::BadUTF8)?;
        Ok(s)
    }
}

impl<'r> ReadXDR<'r> for () {
    fn read_xdr<R: Reader<'r>>(_r: &mut R) -> ReadResult<Self> {
        Ok(())
    }
}

impl<'r> ReadXDR<'r> for Infallible {
    fn read_xdr<R: Reader<'r>>(_r: &mut R) -> ReadResult<Self> {
        unreachable!()
    }
}

macro_rules! read_tuple {
    { $( $t:ident ),+ } => {

        impl<'r, $($t),+> ReadXDR<'r> for ($($t),+)
        where
            $( $t: ReadXDR<'r>, )+
        {
            fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
                Ok(($(
                        { let v: $t = read_xdr(r)?; v }
                        ),+))
            }
        }

    };
}

read_tuple!(T1, T2);
read_tuple!(T1, T2, T3);
read_tuple!(T1, T2, T3, T4);
read_tuple!(T1, T2, T3, T4, T5);
read_tuple!(T1, T2, T3, T4, T5, T6);
read_tuple!(T1, T2, T3, T4, T5, T6, T7);
read_tuple!(T1, T2, T3, T4, T5, T6, T7, T8);
read_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9);
read_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10);
read_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11);
read_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12);
read_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13);
read_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14);
read_tuple!(
    T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15
);
read_tuple!(
    T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16
);

// impl for single-element tuples
impl<'r, T> ReadXDR<'r> for (T,)
where
    T: ReadXDR<'r>,
{
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        Ok((read_xdr(r)?,))
    }
}

impl<'r, T> ReadXDR<'r> for Option<T>
where
    T: ReadXDR<'r>,
{
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        let is_present = r.read_bool()?;
        if is_present {
            Ok(Some(read_xdr(r)?))
        } else {
            Ok(None)
        }
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
impl<'r, T> ReadXDR<'r> for xdr_common::Box<T>
where
    T: ReadXDR<'r>,
{
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        Ok(Self::new(read_xdr(r)?))
    }
}

impl<'r, T, E> ReadXDR<'r> for core::result::Result<T, E>
where
    T: ReadXDR<'r>,
    E: ReadXDR<'r>,
{
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        let d = r.read_i32()?;

        match d {
            0 => Ok(Result::Ok(read_xdr(r)?)),
            1 => Ok(Result::Err(read_xdr(r)?)),
            _ => Err(ReadError::BadDiscriminator(d)),
        }
    }
}

impl<'r, T> ReadXDR<'r> for core::marker::PhantomData<T> {
    fn read_xdr<R: Reader<'r>>(_r: &mut R) -> ReadResult<Self> {
        Ok(core::marker::PhantomData)
    }
}

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
const _: () = {
    use alloc::vec::Vec;

    impl<'r> ReadXDR<'r> for Vec<u8> {
        fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
            let bytes: &[u8] = r.read_opaque_variable(u32::MAX)?;
            let vec = Vec::from(bytes); // copy the slice
            Ok(vec)
        }
    }

    impl<'r, T> ReadXDR<'r> for Vec<T>
    where
        T: ReadXDR<'r>,
    {
        fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
            let len = r.read_u32()? as usize;

            let mut vec: Vec<T> = Vec::with_capacity(len);
            let slice = vec.spare_capacity_mut();

            // Initialize elements one by one, dropping already-initialized elements on error
            for (i, elt) in slice.iter_mut().enumerate() {
                match read_xdr(r) {
                    Ok(val) => {
                        elt.write(val);
                    }
                    Err(e) => {
                        // Drop all initialized elements to avoid memory leak
                        slice.iter_mut().take(i).for_each(|elt| unsafe {
                            elt.assume_init_drop();
                        });
                        return Err(e);
                    }
                }
            }

            // SAFETY: all elts up to `len` have been initialized
            unsafe { vec.set_len(len) }

            Ok(vec)
        }
    }
};

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
const _: () = {
    use xdr_common::*;

    use crate::serialize::reader::{ReadError, Reader};
    use crate::{ReadXDR, read_xdr};

    use alloc::vec::Vec; // can use noalloc machinery in common_types to avoid this

    impl<'r, const MAX: u32> ReadXDR<'r> for BytesM<MAX> {
        fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
            let bytes = r.read_opaque_variable(MAX)?;
            let vec = Vec::from(bytes);
            Ok(Self(vec))
        }
    }

    impl<'r, const MAX: u32> ReadXDR<'r> for StringM<MAX> {
        fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
            let bytes = r.read_opaque_variable(MAX)?;
            let vec = Vec::from(bytes);
            Ok(Self(vec))
        }
    }

    impl<'r, const MAX: u32, T> ReadXDR<'r> for VecM<T, MAX>
    where
        T: ReadXDR<'r>,
    {
        fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
            let len = r.read_u32()? as usize;
            if len > MAX as usize {
                return Err(ReadError::MaxLengthExceeded);
            }

            let mut vec: Vec<T> = Vec::with_capacity(len);
            let slice = vec.spare_capacity_mut();

            for (i, elt) in slice.iter_mut().enumerate() {
                match read_xdr(r) {
                    Ok(val) => {
                        elt.write(val);
                    }
                    Err(e) => {
                        slice.iter_mut().take(i).for_each(|elt| unsafe {
                            elt.assume_init_drop();
                        });
                        return Err(e);
                    }
                }
            }

            // SAFETY: all elts up to `len` have been initialized
            unsafe { vec.set_len(len) }

            Ok(Self(vec))
        }
    }

    // make VecM<u8> work like BytesM;
    impl<'r, const MAX: u32> ReadXDR<'r> for VecM<u8, MAX> {
        fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
            let bytes = r.read_opaque_variable(MAX)?;
            let vec = Vec::from(bytes);
            Ok(Self(vec))
        }
    }
};

#[cfg(not(feature = "alloc"))]
const _: () = {
    use crate::ReadXDR;
    use crate::serialize::reader::Reader;
    use xdr_common::*;

    impl<'r, const MAX: u32> ReadXDR<'r> for BytesM<MAX> {
        fn read_xdr<R: Reader<'r>>(_r: &mut R) -> ReadResult<Self> {
            unimplemented!()
        }
    }

    impl<'r, const MAX: u32> ReadXDR<'r> for StringM<MAX> {
        fn read_xdr<R: Reader<'r>>(_r: &mut R) -> ReadResult<Self> {
            unimplemented!()
        }
    }

    impl<'r, const MAX: u32, T> ReadXDR<'r> for VecM<T, MAX>
    where
        T: ReadXDR<'r> + 'static,
    {
        fn read_xdr<R: Reader<'r>>(_r: &mut R) -> ReadResult<Self> {
            unimplemented!()
        }
    }

    // make VecM<u8> work like BytesM;
    impl<'r, const MAX: u32> ReadXDR<'r> for VecM<u8, MAX> {
        fn read_xdr<R: Reader<'r>>(_r: &mut R) -> ReadResult<Self> {
            unimplemented!()
        }
    }
};
