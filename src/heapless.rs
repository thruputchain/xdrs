use crate::cxs;
use crate::read_xdr::ReadXDR;
use crate::serialize::reader::{ReadError, ReadResult, Reader};
use crate::serialize::writer::{WriteResult, Writer};
use crate::serialize::writer_unchecked::WriterUnchecked;
use crate::txs;
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
            .map_err(|_| ReadError::max_length_exceeded())?;

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
            return Err(ReadError::max_length_exceeded());
        }

        let mut vec = Vec::new();
        for _ in 0..len {
            vec.push(<T as ReadXDR>::read_xdr(r)?)
                .map_err(|_| ReadError::max_length_exceeded())?;
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
        let slice = str::from_utf8(bytes).map_err(|_| ReadError::bad_utf8())?;

        let mut s = String::new();
        s.push_str(slice)
            .map_err(|_| ReadError::max_length_exceeded())?;

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

// -- Worst-case size bounds --
//
// heapless containers store their elements inline (no heap indirection), so
// unlike `VecM<T, MAX>` a recursive type through them is impossible by
// construction, and naming the element's bound cannot form a const-eval
// cycle — the same argument as arrays. Bounds are therefore exact.
//
// A capacity `N` that exceeds `u32::MAX` cannot be length-prefixed, so it
// bounds to `None` (such values can never be fully encoded), mirroring
// `max_size_opaque_fixed`.
//
// As with the read/write impls above, the `Vec<u8, N>` and `Vec<T, N>` impl
// pairs are disjoint because `u8` does not implement the trait.

impl<const N: usize> cxs::ConstMaxSize for Vec<u8, N> {
    const MAX_SIZE: Option<usize> = if N > u32::MAX as usize {
        None
    } else {
        cxs::const_writer::max_size_opaque_variable(N as u32)
    };
}

impl<T: cxs::ConstMaxSize, const N: usize> cxs::ConstMaxSize for Vec<T, N> {
    const MAX_SIZE: Option<usize> = if N > u32::MAX as usize {
        None
    } else {
        cxs::const_writer::max_size_add(
            Some(cxs::const_writer::size_u32(N as u32)),
            cxs::const_writer::max_size_array(N, T::MAX_SIZE),
        )
    };
}

impl<const N: usize> cxs::ConstMaxSize for String<N> {
    const MAX_SIZE: Option<usize> = if N > u32::MAX as usize {
        None
    } else {
        cxs::const_writer::max_size_opaque_variable(N as u32)
    };
}

impl<const N: usize> txs::ConstMaxSize for Vec<u8, N> {
    const MAX_SIZE: Option<usize> = if N > u32::MAX as usize {
        None
    } else {
        txs::const_writer::max_size_opaque_variable(N as u32)
    };
}

impl<T: txs::ConstMaxSize, const N: usize> txs::ConstMaxSize for Vec<T, N> {
    const MAX_SIZE: Option<usize> = if N > u32::MAX as usize {
        None
    } else {
        txs::const_writer::max_size_add(
            Some(txs::const_writer::size_u32(N as u32)),
            txs::const_writer::max_size_array(N, T::MAX_SIZE),
        )
    };
}

impl<const N: usize> txs::ConstMaxSize for String<N> {
    const MAX_SIZE: Option<usize> = if N > u32::MAX as usize {
        None
    } else {
        txs::const_writer::max_size_opaque_variable(N as u32)
    };
}
