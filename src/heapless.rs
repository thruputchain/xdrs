use crate::const_write_xdr::ConstWriteXDR;
use crate::encoded_max;
use crate::read_xdr::ReadXDR;
use crate::serialize::reader::{ReadError, ReadResult, Reader};
use crate::serialize::writer::{WriteResult, Writer};
use crate::valid_u32::*;
use crate::write_xdr::WriteXDR;
use crate::{cxs, txs};

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
}

impl<const N: usize> ConstWriteXDR for Vec<u8, N> {
    const MAX_CXS_LEN: Option<usize> = Some(cxs::const_writer::size_opaque_variable(N));
    const MAX_TXS_LEN: Option<usize> = Some(txs::const_writer::size_opaque_variable(N));
    fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
        let () = ValidU32::<N>::OK;
        w.write_opaque_variable(self.as_slice());
    }
    fn write_txs(&self, w: &mut txs::Cursor<'_>) {
        let () = ValidU32::<N>::OK;
        w.write_opaque_variable(self.as_slice());
    }
    fn cxs_size(&self) -> usize {
        cxs::const_writer::size_opaque_variable(self.len())
    }
    fn txs_size(&self) -> usize {
        txs::const_writer::size_opaque_variable(self.len())
    }
}

impl<T: ConstWriteXDR, const N: usize> ConstWriteXDR for Vec<T, N> {
    const MAX_CXS_LEN: Option<usize> = encoded_max::add(
        Some(cxs::const_writer::size_u32(N as u32)),
        encoded_max::mul(N, T::MAX_CXS_LEN),
    );
    const MAX_TXS_LEN: Option<usize> =
        encoded_max::add(Some(4), encoded_max::mul(N, T::MAX_TXS_LEN));
    fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
        let () = ValidU32::<N>::OK;
        w.write_u32(self.len() as u32);
        for elt in self {
            elt.write_cxs(w);
        }
    }
    fn write_txs(&self, w: &mut txs::Cursor<'_>) {
        let () = ValidU32::<N>::OK;
        w.write_u32(self.len() as u32);
        for elt in self {
            elt.write_txs(w);
        }
    }
    fn cxs_size(&self) -> usize {
        let mut s = cxs::const_writer::size_u32(self.len() as u32);
        for elt in self {
            s += elt.cxs_size();
        }
        s
    }
    fn txs_size(&self) -> usize {
        let mut s = 4;
        for elt in self {
            s += elt.txs_size();
        }
        s
    }
}

impl<const N: usize> ConstWriteXDR for String<N> {
    const MAX_CXS_LEN: Option<usize> = Some(cxs::const_writer::size_opaque_variable(N));
    const MAX_TXS_LEN: Option<usize> = Some(txs::const_writer::size_opaque_variable(N));
    fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
        let () = ValidU32::<N>::OK;
        w.write_opaque_variable(self.as_bytes());
    }
    fn write_txs(&self, w: &mut txs::Cursor<'_>) {
        let () = ValidU32::<N>::OK;
        w.write_opaque_variable(self.as_bytes());
    }
    fn cxs_size(&self) -> usize {
        cxs::const_writer::size_opaque_variable(self.len())
    }
    fn txs_size(&self) -> usize {
        txs::const_writer::size_opaque_variable(self.len())
    }
}
