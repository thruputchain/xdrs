//! Types shaped like generated protocol bindings, derived against
//! `xdr_common` alone (`#[xdr(crate = "xdr_common")]`).
//!
//! If a derive starts emitting a path that only `thruput-xdrs` has (a codec
//! function, a buffer type), this crate stops compiling: that would make a
//! codec release a breaking change for every crate deriving XDR.

use xdr_common::{BytesM, EncodesAs, StringM, VecM};
use xdr_derive::{ConstWriteXDR, ConstXDRSize, ReadXDR, WriteXDR};

/// An extension point, as generated for every protocol struct.
#[derive(Clone, Debug, PartialEq, Eq, ReadXDR, WriteXDR, ConstWriteXDR, ConstXDRSize)]
#[xdr(crate = "xdr_common", discriminant = "i32")]
#[repr(i32)]
pub enum ExtensionPoint {
    /// The only arm so far.
    V0 = 0,
}

/// A struct with fixed, variable-length and boxed fields.
#[derive(Clone, Debug, PartialEq, Eq, ReadXDR, WriteXDR, ConstWriteXDR, ConstXDRSize)]
#[xdr(crate = "xdr_common")]
#[repr(C)]
pub struct Header {
    /// Extension point.
    pub ext: ExtensionPoint,
    /// A fixed-size hash.
    pub hash: [u8; 32],
    /// A bounded label.
    pub label: StringM<16>,
    /// Bounded opaque data.
    pub data: BytesM<64>,
    /// A boxed field, which the const derives reach through `box_as_ref`.
    pub nested: Box<Inner>,
}

/// A small struct behind a `Box`.
#[derive(Clone, Debug, PartialEq, Eq, ReadXDR, WriteXDR, ConstWriteXDR, ConstXDRSize)]
#[xdr(crate = "xdr_common")]
#[repr(C)]
pub struct Inner {
    /// A counter.
    pub n: u64,
    /// A flag.
    pub flag: bool,
}

/// A union switched by an unsigned discriminant, with a default arm.
#[derive(Clone, Debug, PartialEq, Eq, ReadXDR, WriteXDR, ConstXDRSize)]
#[xdr(crate = "xdr_common", discriminant = "u32")]
#[repr(i32)]
pub enum Message {
    /// No payload.
    Ping = 0,
    /// A header.
    Header(Header) = 1,
    /// A batch of counters.
    Counters(VecM<u64, 8>) = 2,
    /// Any other tag, kept as is.
    #[xdr(default_variant)]
    Other(u32),
}

/// A borrowed view, as the derive emits for zero-copy decoding.
#[derive(Clone, Debug, PartialEq, Eq, ReadXDR, WriteXDR)]
#[xdr(crate = "xdr_common")]
pub struct Borrowed<'a> {
    /// Borrowed opaque bytes.
    pub bytes: &'a [u8],
    /// Borrowed string.
    pub text: &'a str,
}

/// A stand-in for a generated RPC client method: the argument is anything
/// that encodes as a [`Header`], so its signature names `EncodesAs` from
/// xdr-common. Returns the number of TXS bytes the argument writes, counted
/// through the `Writer` interface alone.
pub fn send_header(arg: &(impl EncodesAs<Header> + ?Sized)) -> usize {
    let mut counter = TxsSizeCounter::default();
    arg.write_xdr(&mut counter).expect("counting cannot fail");
    counter.0
}

/// A minimal `Writer` that counts TXS bytes, to show that a format can be
/// implemented against xdr-common without the codec crate.
#[derive(Debug, Default)]
pub struct TxsSizeCounter(pub usize);

impl xdr_common::Writer<()> for TxsSizeCounter {
    fn write_opaque_fixed(&mut self, bytes: &[u8]) -> xdr_common::WriteResult<()> {
        self.0 += bytes.len().div_ceil(4) * 4;
        Ok(())
    }
    fn write_opaque_unpadded(&mut self, bytes: &[u8]) -> xdr_common::WriteResult<()> {
        self.0 += bytes.len();
        Ok(())
    }
    fn write_opaque_padding(&mut self, len: usize) -> xdr_common::WriteResult<()> {
        self.0 += len.div_ceil(4) * 4 - len;
        Ok(())
    }
    fn write_opaque_variable(&mut self, bytes: &[u8]) -> xdr_common::WriteResult<()> {
        self.0 += 4;
        self.write_opaque_fixed(bytes)
    }
    fn write_i64(&mut self, _: i64) -> xdr_common::WriteResult<()> {
        self.0 += 8;
        Ok(())
    }
    fn write_i32(&mut self, _: i32) -> xdr_common::WriteResult<()> {
        self.0 += 4;
        Ok(())
    }
    fn write_i16(&mut self, _: i16) -> xdr_common::WriteResult<()> {
        self.0 += 4;
        Ok(())
    }
    fn write_u64(&mut self, _: u64) -> xdr_common::WriteResult<()> {
        self.0 += 8;
        Ok(())
    }
    fn write_u32(&mut self, _: u32) -> xdr_common::WriteResult<()> {
        self.0 += 4;
        Ok(())
    }
    fn write_u16(&mut self, _: u16) -> xdr_common::WriteResult<()> {
        self.0 += 4;
        Ok(())
    }
    fn write_bool(&mut self, _: bool) -> xdr_common::WriteResult<()> {
        self.0 += 4;
        Ok(())
    }
    fn write_f32(&mut self, _: f32) -> xdr_common::WriteResult<()> {
        self.0 += 4;
        Ok(())
    }
    fn write_f64(&mut self, _: f64) -> xdr_common::WriteResult<()> {
        self.0 += 8;
        Ok(())
    }
}
