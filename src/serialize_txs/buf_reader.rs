use crate::serialize::reader::*;

pub struct BufReader<'r> {
    data: &'r [u8],
    pos: usize,
}

macro_rules! read_int {
    [ $($read:ident($nbytes:expr): $ty:ty),* $(,)? ] => { $(
        fn $read(&mut self) -> Result<$ty> {
            let bytes: [u8; $nbytes] = self.get_input($nbytes)?.try_into().unwrap();
            let n = <$ty>::from_be_bytes(bytes);
            Ok(n)
        }
    )* };
}

impl<'a> BufReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        let pos = 0;
        Self { data, pos }
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    fn get_input(&mut self, requested: u32) -> Result<&'a [u8]> {
        let extent = self.pos + requested as usize;
        if extent > self.data.len() {
            // no panic because remaining < requested <= u32::MAX
            let remaining = (self.data.len() - self.pos).try_into().unwrap();
            return Err(Error::SourceEmpty {
                requested,
                remaining,
            });
        }

        let buf = &self.data[self.pos..extent];

        self.pos = extent;

        Ok(buf)
    }

    fn skip_input(&mut self, skip_bytes: u32) -> Result<()> {
        debug_assert!(skip_bytes <= 3);

        let _ = self.get_input(skip_bytes)?;
        Ok(())
    }
}

impl<'r> Reader<'r> for BufReader<'r> {
    fn read_opaque_fixed(&mut self, len: u32) -> Result<&'r [u8]> {
        let buf = self.get_input(len)?;

        let padlen = 4 - (len % 4);
        self.skip_input(padlen)?;

        Ok(buf)
    }

    fn read_opaque_variable(&mut self, max: u32) -> Result<&'r [u8]> {
        let len = self.read_u32()?;

        if len > max {
            return Err(Error::MaxLengthExceeded);
        }

        self.read_opaque_fixed(len)
    }

    read_int! [
        read_u64(8): u64,
        read_u32(4): u32,
        read_u16(2): u16,

        read_i64(8): i64,
        read_i32(4): i32,
        read_i16(2): i16,
    ];
}
