use crate::codec::read::*;

pub trait Construct<'p> {
    fn construct(&mut self, r: &mut dyn Read<'p>) -> Result<()>;
}

impl<'p> Construct<'p> for i32 {
    fn construct(&mut self, r: &mut dyn Read<'p>) -> Result<()> {
        *self = r.read_i32()?;
        Ok(())
    }
}
