use crate::codec::reader::*;
use crate::bytes::*;
use core::mem::MaybeUninit as MU;

pub fn read_xdr<'r, R, T>(r: &mut R) -> Result<T>
where
    R: Reader<'r>,
    T: ReadXDR<'r>
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
    read_u8 for u8,

    read_bool for bool,
}

impl<'r, T> ReadXDR<'r> for Option<T>
where
    T: ReadXDR<'r>
{
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
        let mut value: Option<T> = None;
        let visit_opt = |r: Option<&mut R>| -> Result<()> {
            if let Some(r) = r {
                value = Some(read_xdr(r)?);
            }
            Ok(())
        };
        r.read_optional(visit_opt)?;
        Ok(value)
    }
}

impl<'r, T, const N: usize> ReadXDR<'r> for [T; N]
where
    T: ReadXDR<'r>
{
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
        let mut arr: [MU<T>; N] = [const { MU::uninit() }; N];
        let mut i = 0;
        let visit_element = |r: &mut R| -> Result<()> {
            arr[i].write(read_xdr(r)?);
            i += 1;
            Ok(())
        };

        r.read_array(N, visit_element)?;
        let arr = unsafe { MU::array_assume_init(arr) };

        Ok(arr)
    }
}

impl<'a, 'r: 'a> ReadXDR<'r> for Bytes<'a> {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
        let bytes: &[u8] = r.read_bytes()?;
        Ok(Bytes::from(bytes))
    }
}

impl<'a, 'r: 'a, const N: usize> ReadXDR<'r> for ByteArray<'a, N> {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
        let bytes: &[u8; N] = (r.read_byte_array(N as u32)?).try_into().unwrap();
        Ok(ByteArray::from(bytes))
    }
}

impl<'r, const N: usize> ReadXDR<'r> for OwnedByteArray<N> {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
        let mut arr: [MU<u8>; N] = [const { MU::uninit() }; N];
        r.read_byte_array_into(&mut arr, N as u32)?;
        let arr = unsafe { MU::array_assume_init(arr) };
        Ok(OwnedByteArray::from(arr))
    }
}
