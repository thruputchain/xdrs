//! Implement `ReadXDR` and `WriteXDR` for `i128` and `u128` using encodings `(i64,u64)` and
//! `(u64,u64)`, respectively.

const _: () = {
    use crate::read_xdr::*;
    use crate::serialize::reader::*;

    impl<'r> ReadXDR<'r> for i128 {
        #[inline]
        fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
            let highbits: i64 = read_xdr(r)?;
            let lowbits: u64 = read_xdr(r)?;
            let i: i128 = ((highbits as i128) << 64) | (lowbits as i128);

            Ok(i)
        }
    }

    impl<'r> ReadXDR<'r> for u128 {
        #[inline]
        fn read_xdr<R: Reader<'r>>(r: &mut R) -> ReadResult<Self> {
            let highbits: u64 = read_xdr(r)?;
            let lowbits: u64 = read_xdr(r)?;
            let u: u128 = ((highbits as u128) << 64) | (lowbits as u128);

            Ok(u)
        }
    }
};

const _: () = {
    use crate::serialize::writer::*;
    use crate::serialize::writer_unchecked::*;
    use crate::write_xdr::*;

    impl WriteXDR for i128 {
        #[inline]
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
            let highbits = (*self >> 64) as i64;
            let lowbits = (*self & 0xffffffffffffffff) as u64;

            write_xdr(&highbits, w)?;
            write_xdr(&lowbits, w)
        }

        #[inline]
        unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
            let highbits = (*self >> 64) as i64;
            let lowbits = (*self & 0xffffffffffffffff) as u64;

            // SAFETY: the caller guarantees space for both halves.
            unsafe {
                write_xdr_unchecked(&highbits, w);
                write_xdr_unchecked(&lowbits, w);
            }
        }
    }

    impl WriteXDR for u128 {
        #[inline]
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
            let highbits = (*self >> 64) as u64;
            let lowbits = (*self & 0xffffffffffffffff) as u64;

            write_xdr(&highbits, w)?;
            write_xdr(&lowbits, w)
        }

        #[inline]
        unsafe fn write_xdr_unchecked<F, W: WriterUnchecked<F>>(&self, w: &mut W) {
            let highbits = (*self >> 64) as u64;
            let lowbits = (*self & 0xffffffffffffffff) as u64;

            // SAFETY: the caller guarantees space for both halves.
            unsafe {
                write_xdr_unchecked(&highbits, w);
                write_xdr_unchecked(&lowbits, w);
            }
        }
    }
};
