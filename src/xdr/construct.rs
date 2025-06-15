use crate::codec::read::*;
use core::mem::MaybeUninit as MU;

pub trait Construct<'r>: Sized {
    fn construct(this: &mut MU<Self>, r: &mut dyn Read<'r>) -> Result<()>;
}

impl<'r> Construct<'r> for i32 {
    fn construct(this: &mut MU<Self>, r: &mut dyn Read<'r>) -> Result<()> {
        this.write(r.read_i32()?);
        Ok(())
    }
}

impl<'r> Construct<'r> for u32 {
    fn construct(this: &mut MU<Self>, r: &mut dyn Read<'r>) -> Result<()> {
        this.write(r.read_u32()?);
        Ok(())
    }
}

pub fn construct<'r, T>(
    this: &mut MU<T>,
    r: &mut dyn Read<'r>
) -> Result<()>
where
    T: Construct<'r>
{
    Construct::construct(this, r)
}
