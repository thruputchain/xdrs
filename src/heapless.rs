use crate::read_xdr::ReadXDR;
use crate::serialize::reader::{Error, Reader, Result as ReadResult};
use crate::serialize::writer::{Result as WriteResult, Writer};
use crate::valid_u32::*;
use crate::write_xdr::WriteXDR;

use heapless::String;
use heapless::Vec;

impl<'r, const N: usize> ReadXDR<'r> for Vec<u8, N> {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        let () = ValidU32::<N>::OK;

        let bytes = r.read_opaque_variable(N as u32)?;

        let mut s = Vec::new();
        s.extend_from_slice(bytes).unwrap();

        Ok(s)
    }
}

impl<const N: usize> WriteXDR for Vec<u8, N> {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        let s = self.as_slice();

        s.write_xdr(w)
    }
}

impl<'r, const N: usize> ReadXDR<'r> for String<N> {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        let () = ValidU32::<N>::OK;

        let bytes = r.read_opaque_variable(N as u32)?;
        let slice = str::from_utf8(bytes).map_err(|_| Error::BadUTF8)?;

        let mut s = String::new();
        s.push_str(slice).unwrap();

        Ok(s)
    }
}

impl<const N: usize> WriteXDR for String<N> {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        let s = self.as_str();

        s.write_xdr(w)
    }
}
