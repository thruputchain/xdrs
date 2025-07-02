use crate::serialize::reader::*;
use core::mem::MaybeUninit as MU;
use crate::valid_u32::*;

#[cfg(feature = "alloc")]
use alloc::vec::Vec;

pub fn read_xdr<'r, R, T>(r: &mut R) -> Result<T>
where
    R: Reader<'r>,
    T: ReadXDR<'r>,
{
    ReadXDR::read_xdr(r)
}

/// Types that can be constructed by reading XDR data,
/// possibly sharing data from the input
pub trait ReadXDR<'r>: Sized {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self>;
}

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
    read_i8 for i8,

    read_u64 for u64,
    read_u32 for u32,
    read_u16 for u16,

    read_bool for bool,
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

impl<'r, T, const N: usize> ReadXDR<'r> for [T; N]
where
    T: ReadXDR<'r>,
{
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
        let () = ValidU32::<N>::OK;

        let mut arr: [MU<T>; N] = [const { MU::uninit() }; N];

        for i in 0..N {
            arr[i].write(read_xdr(r)?);
        }

        // SAFETY: all elements have been initialized
        let arr = unsafe { MU::array_assume_init(arr) };

        Ok(arr)
    }
}

impl<'a, 'r: 'a> ReadXDR<'r> for &'a [u8] {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
        r.read_opaque_variable()
    }
}

impl<'a, 'r: 'a, const N: usize> ReadXDR<'r> for &'a [u8; N] {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
        let () = ValidU32::<N>::OK;
        let len = N as u32;

        // PANIC: We depend on `Reader::read_opaque_fixed` returning a slice of the requested size
        let bytes: &[u8; N] = r.read_opaque_fixed(len)?.try_into().unwrap();
        Ok(bytes)
    }
}

impl<'r, const N: usize> ReadXDR<'r> for [u8; N] {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
        let () = ValidU32::<N>::OK;

        let mut arr: [MU<u8>; N] = [const { MU::uninit() }; N];

        r.read_opaque_fixed_into(&mut arr, N as u32)?;

        // SAFETY: We depend on read_opaque_fixed_into to init all elements
        let arr = unsafe { MU::array_assume_init(arr) };

        Ok(arr)
    }
}

#[cfg(feature = "alloc")]
impl<'r> ReadXDR<'r> for alloc::vec::Vec<u8> {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
        let bytes: &[u8] = r.read_opaque_variable()?;
        let vec = alloc::vec::Vec::from(bytes); // copy the slice
        Ok(vec)
    }
}

#[cfg(feature = "alloc")]
impl<'r, T> ReadXDR<'r> for alloc::vec::Vec<T>
where
    T: ReadXDR<'r>
{
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
        let len = r.read_u32()? as usize;

        let mut vec: Vec<T> = Vec::with_capacity(len as usize);
        let slice = vec.spare_capacity_mut();

        for i in 0..len {
            slice[i].write(read_xdr(r)?);
        }

        // SAFETY: all elts up to `len` have been initialized
        unsafe { vec.set_len(len) }

        Ok(vec)
    }
}
