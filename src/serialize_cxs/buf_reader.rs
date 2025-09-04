#[cfg(feature = "strict")]
use super::limits::*;
use crate::serialize::reader::*;

pub struct BufReader<'r, S = ()> {
    data: &'r [u8],
    pos: usize,
    structure: S,
}

impl<'a> BufReader<'a, ()> {
    pub fn new(data: &'a [u8]) -> Self {
        Self::new_structured(data, ())
    }
}

impl<'a, S> BufReader<'a, S>
    where Self: Reader<'a>
{
    pub fn new_structured(data: &'a [u8], structure: S) -> Self {
        Self { data, pos: 0, structure }
    }

    pub fn structure(&self) -> &S {
        &self.structure
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    fn get_input(&mut self, requested: u32) -> Result<&'a [u8]> {
        let extent = self.pos + requested as usize;
        if extent > self.data.len() {
            // no panic can occur because remaining < requested <= u32::MAX
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

    fn read_byte(&mut self) -> Result<u8> {
        let byte = self.get_input(1)?;

        Ok(byte[0])
    }

    fn read_signed(&mut self) -> Result<i64> {
        let h = self.read_byte()?;
        let s = (h & 0xc0) >> 6;
        let v = h & !0xc0;
        let n: i64;

        if v == 63 {
            // The number is encoded in the following bytes

            const N_BYTES_MAP: [usize; 4] = [1, 2, 4, 8];
            let n_bytes = N_BYTES_MAP[s as usize];

            let input = self.read_opaque_fixed(n_bytes as u32)?;

            let mut int_bytes = [0u8; 8];

            let int_dest = &mut int_bytes[0..n_bytes];
            int_dest.copy_from_slice(input);

            if input[n_bytes - 1] & 0x80 != 0 {
                // the high bit is set, so sign-extend remaining bytes
                for b in int_bytes.iter_mut().skip(n_bytes) {
                    *b = 0xff;
                }
            }

            n = i64::from_le_bytes(int_bytes);

            #[cfg(feature = "strict")]
            {
                // validate that the smallest possible encoding was used

                const N_BYTE_REPR_MINMAX: [(i64, i64); 4] = [
                    (IH_MAX as i64 + 1, IH_MIN as i64 - 1),
                    (i8::MAX as i64 + 1, i8::MIN as i64 - 1),
                    (i16::MAX as i64 + 1, i16::MIN as i64 - 1),
                    (i32::MAX as i64 + 1, i32::MIN as i64 - 1),
                ];
                let (repr_min, repr_max) = N_BYTE_REPR_MINMAX[s as usize];

                if n > 0 {
                    if n < repr_min {
                        return Err(Error::NotMinimalEncoding);
                    }
                } else if n > repr_max {
                    return Err(Error::NotMinimalEncoding);
                }
            }
        } else {
            // The number is the 6-bit, one's-complement signed
            // integer in `v`

            if s != 0 {
                return Err(Error::InvalidHeaderByte { h });
            }

            if v & 0b10_0000 != 0 {
                // The sign-bit is set, so obtain the number by inverting
                // and negating:
                n = -(((!v) & 0b11_1111) as i64);
            } else {
                n = v as i64;
            }
        }

        Ok(n)
    }

    fn read_unsigned(&mut self) -> Result<u64> {
        let h = self.read_byte()?;
        let s = (h & 0xc0) >> 6;
        let v = h & !0xc0;

        if v == 63 {
            // The number is encoded in the following bytes

            const N_BYTES_MAP: [usize; 4] = [1, 2, 4, 8];
            let n_bytes = N_BYTES_MAP[s as usize];

            let input = self.read_opaque_fixed(n_bytes as u32)?;

            let mut int_bytes = [0u8; 8];
            let int_dest = &mut int_bytes[0..n_bytes];

            int_dest.copy_from_slice(input);
            let uint = u64::from_le_bytes(int_bytes);

            #[cfg(feature = "strict")]
            {
                // validate that the smallest possible encoding was used

                const N_BYTE_REPR_MIN: [u64; 4] = [
                    63,
                    u8::MAX as u64 + 1,
                    u16::MAX as u64 + 1,
                    u32::MAX as u64 + 1,
                ];
                let repr_min = N_BYTE_REPR_MIN[s as usize];

                if uint < repr_min {
                    return Err(Error::NotMinimalEncoding);
                }
            }

            Ok(uint)
        } else {
            // The number is the 6-bit unsigned integer in `v`

            if s != 0 {
                return Err(Error::InvalidHeaderByte { h });
            }

            Ok(v as u64)
        }
    }
}

macro_rules! read_signed {
    { $( [$f:ident, $ty:ty] ),* $(,)? } => { $(

        fn $f(&mut self) -> Result<$ty> {
            let n = self.read_signed()?;
            let n = n.try_into().map_err(|_| Error::IntegerOutOfRange)?;
            Ok(n)
        }

    )* };
}

macro_rules! read_unsigned {
    { $( [$f:ident, $ty:ty] ),* $(,)? } => { $(

        fn $f(&mut self) -> Result<$ty> {
            let n = self.read_unsigned()?;
            let n = n.try_into().map_err(|_| Error::IntegerOutOfRange)?;
            Ok(n)
        }

    )* };
}

impl<'r, S> Structured<S> for BufReader<'r, S>
    where S: Structured<S>
{
    fn structure(&self) -> &S {
        &self.structure
    }

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

impl<'r, S> Reader<'r> for BufReader<'r, S>
{
    read_signed! {
        [read_i64, i64],
        [read_i32, i32],
        [read_i16, i16],
    }

    read_unsigned! {
        [read_u64, u64],
        [read_u32, u32],
        [read_u16, u16],
    }

    fn read_opaque_fixed(&mut self, len: u32) -> Result<&'r [u8]> {
        self.get_input(len)
    }

    fn read_opaque_variable(&mut self, max: u32) -> Result<&'r [u8]> {
        let len = self.read_u32()?;

        if len > max {
            return Err(Error::MaxLengthExceeded);
        }

        self.read_opaque_fixed(len)
    }
}
