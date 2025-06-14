use crate::codec::provider::*;

pub trait Construct<'p> {
    fn construct(&mut self, provider: &mut dyn Provider<'p>) -> Result<()>;
}

impl<'p> Construct<'p> for i32 {
    fn construct(&mut self, provider: &mut dyn Provider<'p>) -> Result<()> {
        *self = provider.provide_i32()?;
        Ok(())
    }
}
