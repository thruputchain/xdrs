use crate::mu;
use crate::serialize::reader::*;
use crate::valid_u32::*;
use core::mem::MaybeUninit as MU;

pub fn read_xdr<'r, R, T>(r: &mut R) -> Result<T>
where
    R: Reader<'r>,
    T: ReadXDR<'r>,
{
    <T as ReadXDR>::read_xdr(r)
}

/// Types that can be constructed by reading XDR data,
/// possibly sharing data from the input
pub trait ReadXDR<'r>: Sized {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self>;
}

/// Types that can be constructed by reading XDR data
/// of any lifetime
pub trait ReadXDROwned: for<'r> ReadXDR<'r> {}

impl<T> ReadXDROwned for T where T: for<'r> ReadXDR<'r> {}

macro_rules! construct_int_impls {
    { $( $read_method:ident for $int_ty:ty ),* $(,)? } => { $(

        impl<'r> ReadXDR<'r> for $int_ty {
            fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<$int_ty> {
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
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
        let () = ValidU32::<N>::OK;

        let mut arr: [MU<T>; N] = [const { MU::uninit() }; N];

        // TODO: catch errors to avoid memory leaks
        for elt in arr.iter_mut() {
            elt.write(read_xdr(r)?);
        }

        // SAFETY: all elements have been initialized
        let arr = unsafe { mu::array_assume_init(arr) };

        Ok(arr)
    }
}

impl<'r> ReadXDR<'r> for &'r [u8] {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
        r.read_opaque_variable(u32::MAX)
    }
}

impl<'r, const N: usize> ReadXDR<'r> for &'r [u8; N] {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
        let () = ValidU32::<N>::OK;
        let len = N as u32;

        // PANIC: We rely on `Reader::read_opaque_fixed` to return a slice of
        // the requested size
        let bytes: &[u8; N] = r.read_opaque_fixed(len)?.try_into().unwrap();
        Ok(bytes)
    }
}

impl<'r, const N: usize> ReadXDR<'r> for [u8; N] {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
        let () = ValidU32::<N>::OK;

        let mut arr: [MU<u8>; N] = [MU::uninit(); N];

        r.read_opaque_fixed_into(&mut arr, N as u32)?;

        // SAFETY: We rely on `read_opaque_fixed_into` to init all elements
        let arr = unsafe { mu::array_assume_init(arr) };

        Ok(arr)
    }
}

#[cfg(feature = "alloc")]
impl<'r> ReadXDR<'r> for alloc::string::String {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
        let bytes = r.read_opaque_variable(u32::MAX)?;
        let v = alloc::vec::Vec::from(bytes);
        let s = alloc::string::String::from_utf8(v).map_err(|_| Error::BadUTF8)?;
        Ok(s)
    }
}

impl<'r> ReadXDR<'r> for () {
    fn read_xdr<R: Reader<'r>>(_r: &mut R) -> Result<Self> {
        Ok(())
    }
}

macro_rules! read_tuple {
    { $( $t:ident ),+ } => {

        impl<'r, $($t),+> ReadXDR<'r> for ($($t),+)
        where
            $( $t: ReadXDR<'r>, )+
        {
            fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
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

// impl for single-element tuples
impl<'r, T> ReadXDR<'r> for (T,)
where
    T: ReadXDR<'r>,
{
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
        Ok((read_xdr(r)?,))
    }
}

impl<'r, T> ReadXDR<'r> for Option<T>
where
    T: ReadXDR<'r>,
{
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
        let is_present = r.read_bool()?;
        if is_present {
            Ok(Some(read_xdr(r)?))
        } else {
            Ok(None)
        }
    }
}

#[cfg(all(feature = "xdr_common", feature = "alloc"))]
impl<'r, T> ReadXDR<'r> for xdr_common::Box<T>
where
    T: ReadXDR<'r>,
{
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
        Ok(Self::new(read_xdr(r)?))
    }
}

impl<'r, T, E> ReadXDR<'r> for core::result::Result<T, E>
where
    T: ReadXDR<'r>,
    E: ReadXDR<'r>,
{
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
        let d = r.read_i32()?;

        match d {
            0 => Ok(Result::Ok(read_xdr(r)?)),
            1 => Ok(Result::Err(read_xdr(r)?)),
            _ => Err(Error::BadDiscriminator(d)),
        }
    }
}

impl<'r, T> ReadXDR<'r> for core::marker::PhantomData<T> {
    fn read_xdr<R: Reader<'r>>(_r: &mut R) -> Result<Self> {
        Ok(core::marker::PhantomData)
    }
}

#[cfg(feature = "alloc")]
const _: () = {
    use alloc::vec::Vec;

    impl<'r> ReadXDR<'r> for Vec<u8> {
        fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
            let bytes: &[u8] = r.read_opaque_variable(u32::MAX)?;
            let vec = Vec::from(bytes); // copy the slice
            Ok(vec)
        }
    }

    impl<'r, T> ReadXDR<'r> for Vec<T>
    where
        T: ReadXDR<'r>,
    {
        fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
            let len = r.read_u32()? as usize;

            let mut vec: Vec<T> = Vec::with_capacity(len);
            let slice = vec.spare_capacity_mut();

            // TODO: catch errors to avoid memory leaks
            for elt in slice.iter_mut() {
                elt.write(read_xdr(r)?);
            }

            // SAFETY: all elts up to `len` have been initialized
            unsafe { vec.set_len(len) }

            Ok(vec)
        }
    }
};

#[cfg(all(feature = "xdr_common", feature = "alloc"))]
const _: () = {
    use xdr_common::*;

    use crate::serialize::reader::{Error, Reader, Result};
    use crate::{ReadXDR, read_xdr};

    use alloc::vec::Vec; // can use noalloc machinery in common_types to avoid this

    impl<'r, const MAX: u32> ReadXDR<'r> for BytesM<MAX> {
        fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
            let vec: Vec<u8> = read_xdr(r)?;
            if vec.len() > MAX as usize {
                return Err(Error::MaxLengthExceeded);
            }
            Ok(Self(vec))
        }
    }

    impl<'r, const MAX: u32, T> ReadXDR<'r> for VecM<T, MAX>
    where
        T: ReadXDR<'r>,
    {
        fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
            let vec: Vec<T> = read_xdr(r)?;
            if vec.len() > MAX as usize {
                return Err(Error::MaxLengthExceeded);
            }
            Ok(Self(vec))
        }
    }
};

#[cfg(all(feature = "xdr_common", not(feature = "alloc")))]
const _: () = {
    use crate::ReadXDR;
    use crate::serialize::reader::{Reader, Result};
    use xdr_common::*;

    impl<'r, const MAX: u32> ReadXDR<'r> for BytesM<MAX> {
        fn read_xdr<R: Reader<'r>>(_r: &mut R) -> Result<Self> {
            unimplemented!()
        }
    }

    impl<'r, const MAX: u32, T> ReadXDR<'r> for VecM<T, MAX>
    where
        T: ReadXDR<'r>,
    {
        fn read_xdr<R: Reader<'r>>(_r: &mut R) -> Result<Self> {
            unimplemented!()
        }
    }
};
