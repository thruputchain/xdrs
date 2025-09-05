use crate::serialize::reader::*;

pub struct BufReader<'r, S = ()> {
    data: &'r [u8],
    pos: usize,
    structure: S,
}

impl<'r, S> AsRef<S> for BufReader<'r, S> {
    fn as_ref(&self) -> &S {
        &self.structure
    }
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

impl<'a, S> BufReader<'a, S> {
    pub fn new(data: &'a [u8]) -> Self
        where S: Default
    {
        let pos = 0;
        Self { data, pos, structure: Default::default() }
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

    pub fn skip_input(&mut self, skip_bytes: u32) -> Result<()> {
        debug_assert!(skip_bytes <= 3);

        let _ = self.get_input(skip_bytes)?;
        Ok(())
    }
}

impl<'r, S> Reader<'r> for BufReader<'r, S>
    where S: Structured
{
    type State = S;

    fn read_opaque_fixed(&mut self, len: u32) -> Result<&'r [u8]> {
        let buf = self.get_input(len)?;

        let rem = len % 4;
        if rem > 0 {
            let padlen = 4 - rem;
            self.skip_input(padlen)?;
        }

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

impl<'r, S> Structured for BufReader<'r, S>
    where S: Structured
{
    fn seq(&mut self, len: Option<usize>) -> Result<()> {
        self.structure.seq(len)
    }

    fn next(&mut self) -> Result<()> {
        self.structure.next()
    }

    fn end(&mut self) -> Result<()> {
        self.structure.end()
    }
}
