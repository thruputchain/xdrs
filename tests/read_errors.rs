use xdrs::serialize::reader::ReadError;
#[cfg(not(feature = "permissive"))]
use xdrs::serialize::reader::{ReadResult, Reader};
use xdrs::txs;
#[cfg(not(feature = "permissive"))]
use xdrs::{ReadXDR, cxs};

#[cfg(not(feature = "permissive"))]
#[derive(Debug)]
struct BoundedOpaque;

#[cfg(not(feature = "permissive"))]
impl<'r> ReadXDR<'r> for BoundedOpaque {
    fn read_xdr<R: Reader<'r>>(reader: &mut R) -> ReadResult<Self> {
        reader.read_opaque_variable(1)?;
        Ok(Self)
    }
}

#[cfg(not(feature = "permissive"))]
fn malformed_corpus() -> [ReadError; 10] {
    [
        txs::decode::<u32>(&[]).unwrap_err(),
        txs::decode::<u32>(&[0; 8]).unwrap_err(),
        cxs::decode::<u32>(&[0x40]).unwrap_err(),
        txs::decode::<u16>(&[0, 1, 0, 0]).unwrap_err(),
        txs::decode::<bool>(&[0, 0, 0, 2]).unwrap_err(),
        txs::decode::<Result<(), ()>>(&[0, 0, 0, 2]).unwrap_err(),
        txs::decode::<BoundedOpaque>(&[0, 0, 0, 2]).unwrap_err(),
        txs::decode::<&[u8]>(&[0, 0, 0, 1, 0xaa, 0xff, 0, 0]).unwrap_err(),
        txs::decode::<&str>(&[0, 0, 0, 1, 0xff, 0, 0, 0]).unwrap_err(),
        cxs::decode::<u32>(&[0x3f, 1]).unwrap_err(),
    ]
}

#[cfg(not(feature = "permissive"))]
#[test]
fn malformed_inputs_are_rejected() {
    let errors = malformed_corpus();
    assert_diagnostics(&errors);
}

#[cfg(all(feature = "detailed-errors", not(feature = "permissive")))]
fn assert_diagnostics(errors: &[ReadError; 10]) {
    assert!(matches!(errors[0], ReadError::SourceEmpty { .. }));
    assert!(matches!(errors[1], ReadError::SourceRemaining { .. }));
    assert!(matches!(errors[2], ReadError::InvalidHeaderByte { .. }));
    assert!(matches!(errors[3], ReadError::IntegerOutOfRange));
    assert!(matches!(errors[4], ReadError::BadBool));
    assert!(matches!(errors[5], ReadError::BadDiscriminator(2)));
    assert!(matches!(errors[6], ReadError::MaxLengthExceeded));
    assert!(matches!(errors[7], ReadError::NonZeroPadding));
    assert!(matches!(errors[8], ReadError::BadUTF8));
    assert!(matches!(errors[9], ReadError::NotMinimalEncoding));
}

#[cfg(all(not(feature = "detailed-errors"), not(feature = "permissive")))]
fn assert_diagnostics(errors: &[ReadError; 10]) {
    assert_eq!(core::mem::size_of::<ReadError>(), 0);
    for error in errors {
        assert_eq!(error.to_string(), "XDR decoding failed");
    }
}

#[cfg(feature = "derive")]
#[test]
fn derive_uses_configuration_independent_error_construction() {
    #[derive(Debug, xdrs::ReadXDR)]
    #[repr(i32)]
    enum OnlyKnownDiscriminant {
        Known = 0,
    }

    let error = txs::decode::<OnlyKnownDiscriminant>(&[0, 0, 0, 1]).unwrap_err();

    #[cfg(feature = "detailed-errors")]
    assert!(matches!(error, ReadError::BadDiscriminator(1)));
    #[cfg(not(feature = "detailed-errors"))]
    assert_eq!(error.to_string(), "XDR decoding failed");
}
