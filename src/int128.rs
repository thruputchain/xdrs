//! Implement `ReadXDR` and `WriteXDR` for `i128` and `u128`.
//! (The encoding is non-standard because the XDR spec gives none.)

const _: () = {
    use crate::mu;
    use crate::read_xdr::*;
    use crate::serialize::reader::*;
    use core::mem::MaybeUninit as MU;

    impl<'r> ReadXDR<'r> for i128 {
        fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
            let mut uninit = [MU::<u8>::zeroed(); 16];

            let _ = r.read_opaque_variable_into(&mut uninit, 16)?;

            // SAFETY: the pre-zeroed bytes are valid u8's
            let bytes = unsafe { mu::array_assume_init(uninit) };

            Ok(i128::from_le_bytes(bytes))
        }
    }

    impl<'r> ReadXDR<'r> for u128 {
        fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
            let mut uninit = [MU::<u8>::zeroed(); 16];

            let _ = r.read_opaque_variable_into(&mut uninit, 16)?;

            // SAFETY: the pre-zeroed bytes are valid u8's
            let bytes = unsafe { mu::array_assume_init(uninit) };

            Ok(u128::from_le_bytes(bytes))
        }
    }
};

const _: () = {
    use crate::serialize::writer::Result;
    use crate::serialize::writer::*;
    use crate::write_xdr::*;

    impl WriteXDR for i128 {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
            let bytes = self.to_le_bytes();

            let leading_sign_bytes = if *self >= 0 {
                self.leading_zeros()
            } else {
                self.leading_ones()
            } / 8;

            let n_send_bytes = 16 - leading_sign_bytes as usize;
            let buf = &bytes[..n_send_bytes];

            w.write_opaque_variable(buf)
        }
    }

    impl WriteXDR for u128 {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
            let bytes = self.to_le_bytes();

            let leading_zero_bytes = self.leading_zeros() / 8;

            let n_send_bytes = 16 - leading_zero_bytes as usize;
            let buf = &bytes[..n_send_bytes];

            w.write_opaque_variable(buf)
        }
    }
};
