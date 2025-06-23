use crate::codec::reader::*;
use crate::bytes::*;
use core::mem::MaybeUninit as MU;

pub fn read_xdr<'r, R, T>(
    value: &mut MU<T>,
    r: &mut R,
) -> Result<()>
where
    R: Reader<'r>,
    T: ReadXDR<'r>
{
    ReadXDR::read_xdr(value, r)
}

/// Types that can be constructed by reading XDR data,
/// possibly sharing data from the input
pub trait ReadXDR<'r>: Sized {
    fn read_xdr<R: Reader<'r>>(value: &mut MU<Self>, r: &mut R) -> Result<()>;
}

macro_rules! construct_int_impls {
    { $( $read_method:ident for $int_ty:ty ),* $(,)? } => { $(

        impl<'r> ReadXDR<'r> for $int_ty {
            fn read_xdr<R: Reader<'r>>(value: &mut MU<Self>, r: &mut R) -> Result<()> {
                value.write(r.$read_method()?);
                Ok(())
            }
        }

    )* };
}

impl<'r, T> ReadXDR<'r> for Option<T>
where
    T: ReadXDR<'r>
{
    fn read_xdr<R: Reader<'r>>(value: &mut MU<Self>, r: &mut R) -> Result<()> {
        let mut opt = None;
        let visit_some = |r: &mut R| {
            let mut v: MU<T> = MU::uninit();
            read_xdr(&mut v, r)?;
            let v = unsafe { v.assume_init() };
            opt = Some(v);
            Ok(())
        };
        r.read_optional(visit_some)?;

        value.write(opt);
        Ok(())
    }
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
}

impl<'a, 'r: 'a> ReadXDR<'r> for Bytes<'a> {
    fn read_xdr<R: Reader<'r>>(value: &mut MU<Self>, r: &mut R) -> Result<()> {
        value.write(Bytes::from(r.read_bytes()?));
        Ok(())
    }
}

impl<'a, 'r: 'a, const N: usize> ReadXDR<'r> for ByteArray<'a, N> {
    fn read_xdr<R: Reader<'r>>(value: &mut MU<Self>, r: &mut R) -> Result<()> {
        let bytes: &[u8; N] = (r.read_byte_array(N as u32)?).try_into().unwrap();
        value.write(ByteArray::from(bytes));
        Ok(())
    }
}

impl<'r, const N: usize> ReadXDR<'r> for OwnedByteArray<N> {
    fn read_xdr<R: Reader<'r>>(value: &mut MU<Self>, r: &mut R) -> Result<()> {
        r.read_byte_array_into(value.as_bytes_mut(), N as u32)
    }
}
