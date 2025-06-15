use crate::codec::read::*;
use core::mem::MaybeUninit as MU;

/// Types that can be constructed by reading XDR data
pub trait Construct<'r>: Sized {
    fn construct(value: &mut MU<Self>, r: &mut dyn Read<'r>) -> Result<()>;
}

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

pub fn construct<'r, T>(
    value: &mut MU<T>,
    r: &mut dyn Read<'r>
) -> Result<()>
where
    T: Construct<'r>
{
    Construct::construct(value, r)
}
