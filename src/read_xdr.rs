use crate::mu;
use crate::serialize::reader::*;
use crate::valid_u32::*;
use core::mem::MaybeUninit as MU;

/// Types that can be constructed by reading XDR data,
/// possibly sharing data from the input
pub trait ReadXDR<'r, S = ()>: Sized {
    fn read_xdr<R>(r: &mut R) -> Result<Self>
        where R: Reader<'r, S>;
}

pub fn read_xdr<'r, R, S, T>(r: &mut R) -> Result<T>
where
    R: Reader<'r, S>,
    T: ReadXDR<'r, S>,
{
    T::read_xdr(r)
}

/// Types that can be constructed by reading XDR data
/// of any lifetime
pub trait ReadXDROwned<S = ()>: for<'r> ReadXDR<'r, S> {}

impl<S, T> ReadXDROwned<S> for T
    where T: for<'r> ReadXDR<'r, S> {}

macro_rules! construct_int_impls {
    { $( $read_method:ident for $int_ty:ty ),* $(,)? } => { $(

        impl<'r, S> ReadXDR<'r, S> for $int_ty {
            fn read_xdr<R: Reader<'r, S>>(r: &mut R) -> Result<$int_ty> {
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

#[cfg(all(feature = "extras", not(feature = "int128_xdr")))]
const _: () = {
    impl<'r, S> ReadXDR<'r, S> for i128 {
        fn read_xdr<R: Reader<'r, S>>(r: &mut R) -> Result<Self> {
            let mut uninit = [MU::<u8>::zeroed(); 16];

            let _ = r.read_opaque_variable_into(&mut uninit, 16)?;

            // SAFETY: the pre-zeroed bytes are valid u8's
            let bytes = unsafe { mu::array_assume_init(uninit) };
            Ok(Self::from_le_bytes(bytes))
        }
    }

    impl<'r, S> ReadXDR<'r, S> for u128 {
        fn read_xdr<R: Reader<'r, S>>(r: &mut R) -> Result<Self> {
            let mut uninit = [MU::<u8>::zeroed(); 16];

            let _ = r.read_opaque_variable_into(&mut uninit, 16)?;

            // SAFETY: the pre-zeroed bytes are valid u8's
            let bytes = unsafe { mu::array_assume_init(uninit) };
            Ok(Self::from_le_bytes(bytes))
        }
    }
};

// Another way to serialize 128-bit ints
#[cfg(all(feature = "extras", feature = "int128_xdr"))]
const _: () = {
    impl<'r, S> ReadXDR<'r, S> for i128 {
        fn read_xdr<R: Reader<'r, S>>(r: &mut R) -> Result<Self> {
            let highbits: i64 = read_xdr(r)?;
            let lowbits: u64 = read_xdr(r)?;
            let i: i128 = ((highbits as i128) << 64) | (lowbits as i128);

            Ok(i)
        }
    }

    impl<'r, S> ReadXDR<'r, S> for u128 {
        fn read_xdr<R: Reader<'r, S>>(r: &mut R) -> Result<Self> {
            let highbits: u64 = read_xdr(r)?;
            let lowbits: u64 = read_xdr(r)?;
            let u: u128 = ((highbits as u128) << 64) | (lowbits as u128);

            Ok(u)
        }
    }
};

impl<'r, S, T, const N: usize> ReadXDR<'r, S> for [T; N]
where
    T: ReadXDR<'r, S>,
{
    fn read_xdr<R: Reader<'r, S>>(r: &mut R) -> Result<Self> {
        let () = ValidU32::<N>::OK;

        let mut arr: [MU<T>; N] = [const { MU::uninit() }; N];

        r.seq(Some(N))?;

        // TODO: catch errors to avoid memory leaks
        for elt in arr.iter_mut() {
            elt.write(read_xdr(r)?);
            r.next()?;
        }

        r.end()?;

        // SAFETY: all elements have been initialized
        let arr = unsafe { mu::array_assume_init(arr) };
        Ok(arr)
    }
}

impl<'r, S, const N: usize> ReadXDR<'r, S> for [u8; N] {
    fn read_xdr<R: Reader<'r, S>>(r: &mut R) -> Result<Self> {
        let () = ValidU32::<N>::OK;

        let mut arr: [MU<u8>; N] = [MU::uninit(); N];

        r.read_opaque_fixed_into(&mut arr, N as u32)?;

        // SAFETY: We rely on `read_opaque_fixed_into` to init all elements
        let arr = unsafe { mu::array_assume_init(arr) };

        Ok(arr)
    }
}

impl<'r, S, const N: usize> ReadXDR<'r, S> for &'r [u8; N] {
    fn read_xdr<R: Reader<'r, S>>(r: &mut R) -> Result<Self> {
        let () = ValidU32::<N>::OK;
        let len = N as u32;

        // PANIC: We rely on `Reader::read_opaque_fixed` to return a slice of
        // the requested size
        let bytes: &[u8; N] = r.read_opaque_fixed(len)?.try_into().unwrap();
        Ok(bytes)
    }
}

impl<'r, S> ReadXDR<'r, S> for &'r [u8] {
    fn read_xdr<R: Reader<'r, S>>(r: &mut R) -> Result<Self> {
        r.read_opaque_variable(u32::MAX)
    }
}

#[cfg(feature = "alloc")]
impl<'r, S> ReadXDR<'r, S> for alloc::string::String {
    fn read_xdr<R: Reader<'r, S>>(r: &mut R) -> Result<Self> {
        let bytes = r.read_opaque_variable(u32::MAX)?;
        let v = alloc::vec::Vec::from(bytes);
        let s = alloc::string::String::from_utf8(v).map_err(|_| Error::BadUTF8)?;
        Ok(s)
    }
}

impl<'r, S> ReadXDR<'r, S> for () {
    fn read_xdr<R: Reader<'r, S>>(_r: &mut R) -> Result<Self> {
        Ok(())
    }
}

macro_rules! read_tuple {
    { $n:expr => $( $t:ident ),+ } => {

        impl<'r, S, $($t),+> ReadXDR<'r, S> for ($($t),+)
        where
            $( $t: ReadXDR<'r, S>, )+
        {
            fn read_xdr<R: Reader<'r, S>>(r: &mut R) -> Result<Self>
            {
                r.seq(Some($n))?;
                let v = ( $({ let v: $t = read_xdr(r)?; r.next()?; v }),+ );
                r.end()?;
                Ok(v)
            }
        }

    };
}

read_tuple!(2 => T1, T2);
read_tuple!(3 => T1, T2, T3);
read_tuple!(4 => T1, T2, T3, T4);
read_tuple!(5 => T1, T2, T3, T4, T5);
read_tuple!(6 => T1, T2, T3, T4, T5, T6);

impl<'r, S, T> ReadXDR<'r, S> for Option<T>
where
    T: ReadXDR<'r, S>,
{
    fn read_xdr<R: Reader<'r, S>>(r: &mut R) -> Result<Self> {
        let is_present = r.read_bool()?;
        if is_present {
            Ok(Some(read_xdr(r)?))
        } else {
            Ok(None)
        }
    }
}

#[cfg(all(feature = "xdr_common", feature = "alloc"))]
impl<'r, S, T> ReadXDR<'r, S> for xdr_common::Box<T>
where
    T: ReadXDR<'r, S>,
{
    fn read_xdr<R: Reader<'r, S>>(r: &mut R) -> Result<Self> {
        Ok(Self::new(read_xdr(r)?))
    }
}

impl<'r, S, T, E> ReadXDR<'r, S> for core::result::Result<T, E>
where
    T: ReadXDR<'r, S>,
    E: ReadXDR<'r, S>,
{
    fn read_xdr<R: Reader<'r, S>>(r: &mut R) -> Result<Self> {
        let d = r.read_i32()?;

        match d {
            0 => Ok(Result::Ok(read_xdr(r)?)),
            1 => Ok(Result::Err(read_xdr(r)?)),
            _ => Err(Error::BadDiscriminator(d)),
        }
    }
}

impl<'r, S, T> ReadXDR<'r, S> for core::marker::PhantomData<T> {
    fn read_xdr<R: Reader<'r, S>>(_r: &mut R) -> Result<Self> {
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
            r.seq(Some(len))?;

            let mut vec: Vec<T> = Vec::with_capacity(len);
            let slice = vec.spare_capacity_mut();

            // TODO: catch errors to avoid memory leaks
            for elt in slice.iter_mut() {
                elt.write(read_xdr(r)?);
                r.next()?;
            }

            r.end()?;

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

    impl<'r, T, const MAX: u32> ReadXDR<'r> for VecM<T, MAX>
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

    impl<'r, S, const MAX: u32> ReadXDR<'r, S> for BytesM<MAX> {
        fn read_xdr<R: Reader<'r, S>>(_r: &mut R) -> Result<Self> {
            unimplemented!()
        }
    }

    impl<'r, S, const MAX: u32, T> ReadXDR<'r, S> for VecM<T, MAX>
    where
        T: ReadXDR<'r, S>,
    {
        fn read_xdr<R: Reader<'r, S>>(_r: &mut R) -> Result<Self> {
            unimplemented!()
        }
    }
};
