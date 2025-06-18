use crate::codec::read::*;
use crate::bytes::*;
use core::mem::MaybeUninit as MU;

/// Types that can be constructed by reading XDR data,
/// possibly sharing data from the input
pub trait Construct<'r>: Sized {
    fn construct(value: &mut MU<Self>, r: &mut dyn Read<'r>) -> Result<()>;
}

pub trait ConstructOwned: for<'r> Construct<'r> {}

impl<T> ConstructOwned for T where T: for<'r> Construct<'r> {}

impl<'r> Construct<'r> for i32 {
    fn construct(value: &mut MU<Self>, r: &mut dyn Read<'r>) -> Result<()> {
        value.write(r.read_i32()?);
        Ok(())
    }
}

impl<'r> Construct<'r> for u32 {
    fn construct(value: &mut MU<Self>, r: &mut dyn Read<'r>) -> Result<()> {
        value.write(r.read_u32()?);
        Ok(())
    }
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

pub fn construct<'r, T>(
    value: &mut MU<T>,
    r: &mut dyn Read<'r>
) -> Result<()>
where
    T: Construct<'r>
{
    Construct::construct(value, r)
}
