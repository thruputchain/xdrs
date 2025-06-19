use crate::codec::read::*;
use crate::bytes::*;
use core::mem::MaybeUninit as MU;

pub fn construct<'r, T>(
    value: &mut MU<T>,
    r: &mut dyn Read<'r>
) -> Result<()>
where
    T: Construct<'r>
{
    Construct::construct(value, r)
}

/// Types that can be constructed by reading XDR data,
/// possibly sharing data from the input
pub trait Construct<'r>: Sized {
    fn construct(value: &mut MU<Self>, r: &mut dyn Read<'r>) -> Result<()>;
}

pub trait ConstructOwned: for<'r> Construct<'r> {}

impl<T> ConstructOwned for T where T: for<'r> Construct<'r> {}

macro_rules! construct_int_impls {
    { $( $read_method:ident for $int_ty:ty ),* $(,)? } => { $(

        impl<'r> Construct<'r> for $int_ty {
            fn construct(value: &mut MU<Self>, r: &mut dyn Read<'r>) -> Result<()> {
                value.write(r.$read_method()?);
                Ok(())
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
}

impl<'a, 'r: 'a> Construct<'r> for Bytes<'a> {
    fn construct(value: &mut MU<Self>, r: &mut dyn Read<'r>) -> Result<()> {
        value.write(Bytes::from(r.read_bytes()?));
        Ok(())
    }
}

impl<'a, 'r: 'a, const N: usize> Construct<'r> for ByteArray<'a, N> {
    fn construct(value: &mut MU<Self>, r: &mut dyn Read<'r>) -> Result<()> {
        let bytes: &[u8; N] = (r.read_byte_array(N as u32)?).try_into().unwrap();
        value.write(ByteArray::from(bytes));
        Ok(())
    }
}

impl<'r, const N: usize> Construct<'r> for OwnedByteArray<N> {
    fn construct(value: &mut MU<Self>, r: &mut dyn Read<'r>) -> Result<()> {
        r.read_byte_array_into(value.as_bytes_mut(), N as u32)
    }
}
