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
    use crate::const_write_xdr::ConstWriteXDR;
    use crate::{cxs, txs};

    impl ConstWriteXDR for i128 {
        const MAX_CXS_LEN: Option<usize> = Some(18);
        const MAX_TXS_LEN: Option<usize> = Some(16);
        #[inline]
        fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
            w.write_i128(*self)
        }
        #[inline]
        fn write_txs(&self, w: &mut txs::Cursor<'_>) {
            w.write_i128(*self)
        }
        #[inline]
        fn cxs_size(&self) -> usize {
            cxs::const_writer::size_i128(*self)
        }
        #[inline]
        fn txs_size(&self) -> usize {
            txs::const_writer::size_i128(*self)
        }
    }

    impl ConstWriteXDR for u128 {
        const MAX_CXS_LEN: Option<usize> = Some(18);
        const MAX_TXS_LEN: Option<usize> = Some(16);
        #[inline]
        fn write_cxs(&self, w: &mut cxs::Cursor<'_>) {
            w.write_u128(*self)
        }
        #[inline]
        fn write_txs(&self, w: &mut txs::Cursor<'_>) {
            w.write_u128(*self)
        }
        #[inline]
        fn cxs_size(&self) -> usize {
            cxs::const_writer::size_u128(*self)
        }
        #[inline]
        fn txs_size(&self) -> usize {
            txs::const_writer::size_u128(*self)
        }
    }
};

const _: () = {
    use crate::serialize::writer::*;
    use crate::write_xdr::*;

    impl WriteXDR for i128 {
        #[inline]
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> WriteResult<()> {
            let highbits = (*self >> 64) as i64;
            let lowbits = (*self & 0xffffffffffffffff) as u64;

            write_xdr(&highbits, w)?;
            write_xdr(&lowbits, w)
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
    }
};
