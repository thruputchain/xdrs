//! An alternative encoding for `i128` and `u128`

pub struct I128Alt(pub i128);
pub struct U128Alt(pub u128);

const _: () = {
    use crate::read_xdr::*;
    use crate::mu;
    use crate::serialize::reader::*;
    use core::mem::MaybeUninit as MU;

    impl<'r> ReadXDR<'r> for I128Alt {
        fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
            let mut uninit = [MU::<u8>::zeroed(); 16];

            let _ = r.read_opaque_variable_into(&mut uninit, 16)?;

            // SAFETY: the pre-zeroed bytes are valid u8's
            let bytes = unsafe { mu::array_assume_init(uninit) };

            Ok(I128Alt(i128::from_le_bytes(bytes)))
        }
    }

    impl<'r> ReadXDR<'r> for U128Alt {
        fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
            let mut uninit = [MU::<u8>::zeroed(); 16];

            let _ = r.read_opaque_variable_into(&mut uninit, 16)?;

            // SAFETY: the pre-zeroed bytes are valid u8's
            let bytes = unsafe { mu::array_assume_init(uninit) };

            Ok(U128Alt(u128::from_le_bytes(bytes)))
        }
    }
};

const _: () = {
    use crate::serialize::writer::Result;
    use crate::serialize::writer::*;
    use crate::write_xdr::*;

    impl WriteXDR for I128Alt {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
            let bytes = self.0.to_le_bytes();

            let leading_sign_bytes = if self.0 >= 0 {
                self.0.leading_zeros()
            } else {
                self.0.leading_ones()
            } / 8;

            let n_send_bytes = 16 - leading_sign_bytes as usize;
            let buf = &bytes[..n_send_bytes];

            w.write_opaque_variable(buf)
        }
    }

    impl WriteXDR for U128Alt {
        fn write_xdr<F, W: Writer<F>>(&self, w: &mut W) -> Result<()> {
            let bytes = self.0.to_le_bytes();

            let leading_zero_bytes = self.0.leading_zeros() / 8;

            let n_send_bytes = 16 - leading_zero_bytes as usize;
            let buf = &bytes[..n_send_bytes];

            w.write_opaque_variable(buf)
        }
    }
};
