#![allow(dead_code)]

use core::fmt::Debug;
use core::mem::MaybeUninit as MU;
use xdrs::*;

pub trait Format {
    fn name() -> &'static str;

    fn decode<'r, 'a: 'r, T: ReadXDR<'r>>(buf: &'a [u8]) -> Result<T, ReadError>;
    fn encode_into<T: WriteXDR + ?Sized>(
        value: &T,
        buf: &mut [MU<u8>],
    ) -> Result<usize, WriteError>;

    /// Encode `value` into `buf` using the format's unchecked path.
    ///
    /// # Safety
    ///
    /// `buf.len()` must equal the exact encoded size of `value` (or be
    /// larger), and `value` must respect all declared XDR bounds. In
    /// practice the test helper sizes the buffer from a prior successful
    /// checked encode, which satisfies both requirements.
    unsafe fn encode_into_unchecked<T: WriteXDR + ?Sized>(value: &T, buf: &mut [MU<u8>]) -> usize;

    #[cfg(feature = "alloc")]
    fn encode<T: WriteXDR + ?Sized>(value: &T) -> Result<Vec<u8>, WriteError>;

    // Allocate a buffer of the expected size and encode
    // the value into it
    #[cfg(feature = "alloc")]
    fn encode_into_alloc<T>(value: &T, expected_len: usize) -> Vec<u8>
    where
        T: WriteXDR + ?Sized,
    {
        let mut encoded = Vec::with_capacity(expected_len);
        let written = Self::encode_into(value, encoded.spare_capacity_mut()).unwrap();

        // SAFETY:
        // - the new len must be less than or equal to capacity().
        // - the elements at old_len..new_len must be initialized.
        assert!(written <= encoded.capacity());
        unsafe {
            encoded.set_len(written);
        }

        encoded
    }
}

#[allow(clippy::upper_case_acronyms)]
pub struct CXS;

impl Format for CXS {
    fn name() -> &'static str {
        "CXS"
    }

    fn decode<'r, 'a: 'r, T: ReadXDR<'r>>(buf: &'a [u8]) -> Result<T, ReadError> {
        cxs::decode(buf)
    }

    fn encode_into<T: WriteXDR + ?Sized>(
        value: &T,
        buf: &mut [MU<u8>],
    ) -> Result<usize, WriteError> {
        cxs::encode_into(value, buf)
    }

    unsafe fn encode_into_unchecked<T: WriteXDR + ?Sized>(value: &T, buf: &mut [MU<u8>]) -> usize {
        // SAFETY: forwarded caller contract.
        unsafe { cxs::encode_into_unchecked(value, buf) }
    }

    #[cfg(feature = "alloc")]
    fn encode<T: WriteXDR + ?Sized>(value: &T) -> Result<Vec<u8>, WriteError> {
        cxs::encode(value)
    }
}

#[allow(clippy::upper_case_acronyms)]
pub struct TXS;

impl Format for TXS {
    fn name() -> &'static str {
        "TXS"
    }

    fn decode<'r, 'a: 'r, T: ReadXDR<'r>>(buf: &'a [u8]) -> Result<T, ReadError> {
        txs::decode(buf)
    }

    fn encode_into<T: WriteXDR + ?Sized>(
        value: &T,
        buf: &mut [MU<u8>],
    ) -> Result<usize, WriteError> {
        txs::encode_into(value, buf)
    }

    unsafe fn encode_into_unchecked<T: WriteXDR + ?Sized>(value: &T, buf: &mut [MU<u8>]) -> usize {
        // SAFETY: forwarded caller contract.
        unsafe { txs::encode_into_unchecked(value, buf) }
    }

    #[cfg(feature = "alloc")]
    fn encode<T: WriteXDR + ?Sized>(value: &T) -> Result<Vec<u8>, WriteError> {
        txs::encode(value)
    }
}

pub trait Check: Format {
    fn check_decode<'r, T>(val: &T, expected: &'r [u8])
    where
        T: Debug + Eq + ReadXDR<'r>,
    {
        let val2: T = Self::decode(expected).unwrap_or_else(|e| panic!("decode failed: {e}"));
        assert_eq!(&val2, val);
    }

    fn check_codec<'r, T>(val: &T, expected: &'r [u8])
    where
        T: Debug + Eq + ReadXDR<'r> + WriteXDR,
    {
        Self::check_encode(val, expected);
        Self::check_decode(val, expected);
    }

    fn check_encode<T>(val: &T, expected: &[u8])
    where
        T: WriteXDR + ?Sized,
    {
        // Check the allocating encoder
        #[cfg(feature = "alloc")]
        Self::check_encode_alloc(val, expected);

        // Check the static encoder
        Self::check_encode_into_static(val, expected);
    }

    #[cfg(feature = "alloc")]
    fn check_encode_alloc<T>(value: &T, expected: &[u8])
    where
        T: WriteXDR + ?Sized,
    {
        let encoded = Self::encode(value).unwrap();
        Self::check_eq(&encoded, expected);
    }

    fn check_encode_into_static<T>(value: &T, expected: &[u8])
    where
        T: WriteXDR + ?Sized,
    {
        const BUFSZ: usize = 1000;
        assert!(expected.len() <= BUFSZ);

        // An "uninitialized" buffer that has actually been zeroed
        let mut buf: [MU<u8>; BUFSZ] = [const { MU::zeroed() }; BUFSZ];

        let written = Self::encode_into(value, &mut buf).unwrap();

        // SAFETY: `encode_into` just initialized the first `written` bytes.
        let encoded = unsafe { buf[..written].assume_init_ref() };

        Self::check_eq(encoded, expected);

        // Differential unchecked leg: the checked encode succeeded above,
        // so `written` is the exact encoded byte count and all declared XDR
        // bounds on `value` were validated. We slice the unchecked buffer to
        // exactly `written` bytes to satisfy the tight buffer-size contract.
        let mut ubuf: [MU<u8>; BUFSZ] = [const { MU::zeroed() }; BUFSZ];
        // SAFETY: `ubuf[..written]` is sized to the exact encoded length
        // measured by the checked encode that just succeeded; the successful
        // checked encode also validated every declared XDR bound on `value`.
        let uwritten = unsafe { Self::encode_into_unchecked(value, &mut ubuf[..written]) };
        assert_eq!(
            written,
            uwritten,
            "unchecked written != checked written (format = {})",
            Self::name()
        );
        // SAFETY: `encode_into_unchecked` just initialized the first
        // `uwritten` bytes.
        let uencoded = unsafe { ubuf[..uwritten].assume_init_ref() };
        Self::check_eq(uencoded, expected);
    }

    #[cfg(feature = "alloc")]
    fn check_encode_into_alloc<T>(value: &T, expected: &[u8])
    where
        T: WriteXDR + ?Sized,
    {
        let encoded = Self::encode_into_alloc(value, expected.len());
        Self::check_eq(&encoded, expected);
    }

    // panic with readable output
    fn check_eq(encoded: &[u8], expected: &[u8]) {
        // assert!(a.len() == b.len());

        let a_hex = hex::encode(encoded);
        let b_hex = hex::encode(expected);

        assert_eq!(a_hex, b_hex, "format = {}", Self::name());
    }
}

impl Check for CXS {}
impl Check for TXS {}

#[macro_export]
macro_rules! hex {
    ($hstr:expr) => {
        ::hex::decode($hstr).unwrap_or_else(|e| panic!("Bad hex string: {e}"))
    };
}
