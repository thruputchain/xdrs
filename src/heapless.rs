use crate::read_xdr::ReadXDR;
use crate::serialize::reader::{ReadError, ReadResult, Reader};
use crate::serialize::writer::{WriteResult, Writer};
use crate::serialize::writer_unchecked::WriterUnchecked;
use crate::valid_u32::*;
use crate::write_xdr::WriteXDR;

use heapless::String;
use heapless::Vec;

impl<'r, const N: usize> ReadXDR<'r> for Vec<u8, N> {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        let () = ValidU32::<N>::OK;

        let bytes = r.read_opaque_variable(N as u32)?;

        let mut s = Vec::new();
        s.extend_from_slice(bytes)
            .map_err(|_| ReadError::MaxLengthExceeded)?;

        Ok(s)
    }
}

impl<const N: usize> WriteXDR for Vec<u8, N> {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        let () = ValidU32::<N>::OK;

        w.write_opaque_variable(self.as_slice())
    }

    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        let () = ValidU32::<N>::OK;

        // SAFETY: a heapless Vec<u8, N> never exceeds N <= u32::MAX
        // elements; the caller guarantees buffer space.
        unsafe { w.write_opaque_variable(self.as_slice()) }
    }
}

impl<'r, T, const N: usize> ReadXDR<'r> for Vec<T, N>
where
    T: ReadXDR<'r>,
{
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        let () = ValidU32::<N>::OK;

        let len = r.read_u32()? as usize;
        if len > N {
            return Err(ReadError::MaxLengthExceeded);
        }

        let mut vec = Vec::new();
        for _ in 0..len {
            vec.push(<T as ReadXDR>::read_xdr(r)?)
                .map_err(|_| ReadError::MaxLengthExceeded)?;
        }

        Ok(vec)
    }
}

impl<T, const N: usize> WriteXDR for Vec<T, N>
where
    T: WriteXDR,
{
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        let () = ValidU32::<N>::OK;

        w.write_u32(self.len() as u32)?;
        for elt in self {
            elt.write_xdr(w)?;
        }
        Ok(())
    }

    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        let () = ValidU32::<N>::OK;

        // SAFETY: the caller guarantees space for the length prefix and
        // every element.
        unsafe {
            w.write_u32(self.len() as u32);
            for elt in self {
                elt.write_xdr_unchecked(w);
            }
        }
    }
}

impl<'r, const N: usize> ReadXDR<'r> for String<N> {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
        let () = ValidU32::<N>::OK;

        let bytes = r.read_opaque_variable(N as u32)?;
        let slice = str::from_utf8(bytes).map_err(|_| ReadError::BadUTF8)?;

        let mut s = String::new();
        s.push_str(slice)
            .map_err(|_| ReadError::MaxLengthExceeded)?;

        Ok(s)
    }
}

impl<const N: usize> WriteXDR for String<N> {
    fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
        let () = ValidU32::<N>::OK;

        w.write_opaque_variable(self.as_bytes())
    }

    unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
        let () = ValidU32::<N>::OK;

        // SAFETY: a heapless String<N> never exceeds N <= u32::MAX bytes;
        // the caller guarantees buffer space.
        unsafe { w.write_opaque_variable(self.as_bytes()) }
    }
}
