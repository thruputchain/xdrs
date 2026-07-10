use core::mem::MaybeUninit;
use xdrs::ConstWriteXDR;

fn reference_cxs<T: xdrs::WriteXDR>(v: &T) -> ([u8; 64], usize) {
    let mut mu = [MaybeUninit::<u8>::uninit(); 64];
    let n = xdrs::cxs::encode_into(v, &mut mu).expect("reference encode failed");
    let mut out = [0u8; 64];
    // SAFETY: encode_into guarantees that the first n bytes of `mu` are initialized.
    let mut i = 0;
    while i < n {
        out[i] = unsafe { mu[i].assume_init() };
        i += 1;
    }
    (out, n)
}

fn reference_txs<T: xdrs::WriteXDR>(v: &T) -> ([u8; 64], usize) {
    let mut mu = [MaybeUninit::<u8>::uninit(); 64];
    let n = xdrs::txs::encode_into(v, &mut mu).expect("reference encode failed");
    let mut out = [0u8; 64];
    // SAFETY: encode_into guarantees that the first n bytes of `mu` are initialized.
    let mut i = 0;
    while i < n {
        out[i] = unsafe { mu[i].assume_init() };
        i += 1;
    }
    (out, n)
}

/// Assert: trait encode == WriteXDR encode, exact sizes match, and both
/// sizes are within the declared MAX (when bounded) — for both formats.
fn assert_trait_matches<T: ConstWriteXDR + xdrs::WriteXDR + core::fmt::Debug>(v: &T) {
    let (exp, len) = reference_cxs(v);
    let mut buf = [0u8; 64];
    let mut c = xdrs::cxs::Cursor::new(&mut buf);
    v.write_cxs(&mut c);
    assert_eq!(c.as_bytes(), &exp[..len], "cxs bytes for {v:?}");
    assert_eq!(v.cxs_size(), len, "cxs_size for {v:?}");
    if let Some(m) = T::MAX_CXS_LEN {
        assert!(len <= m, "cxs MAX for {v:?}");
    }

    let (exp, len) = reference_txs(v);
    let mut buf = [0u8; 64];
    let mut c = xdrs::txs::Cursor::new(&mut buf);
    v.write_txs(&mut c);
    assert_eq!(c.as_bytes(), &exp[..len], "txs bytes for {v:?}");
    assert_eq!(v.txs_size(), len, "txs_size for {v:?}");
    if let Some(m) = T::MAX_TXS_LEN {
        assert!(len <= m, "txs MAX for {v:?}");
    }
}

#[test]
fn primitives_match_reference() {
    for v in [
        0i32,
        1,
        -1,
        31,
        32,
        -31,
        -32,
        62,
        63,
        127,
        128,
        255,
        256,
        32767,
        32768,
        65535,
        65536,
        i32::MAX,
        i32::MIN,
    ] {
        assert_trait_matches(&v);
    }
    for v in [0u32, 63, 256, 65536, u32::MAX] {
        assert_trait_matches(&v);
    }
    for v in [0i64, i64::MAX, i64::MIN] {
        assert_trait_matches(&v);
    }
    for v in [0u64, u32::MAX as u64 + 1, u64::MAX] {
        assert_trait_matches(&v);
    }
    for v in [0i16, -32, i16::MAX, i16::MIN] {
        assert_trait_matches(&v);
    }
    for v in [0u16, 63, u16::MAX] {
        assert_trait_matches(&v);
    }
    assert_trait_matches(&true);
    assert_trait_matches(&false);
    assert_trait_matches(&1.5f32);
    assert_trait_matches(&-2.75f64);
}

#[test]
fn primitive_max_values() {
    assert_eq!(<u32 as ConstWriteXDR>::MAX_CXS_LEN, Some(5));
    assert_eq!(<u32 as ConstWriteXDR>::MAX_TXS_LEN, Some(4));
    assert_eq!(<u64 as ConstWriteXDR>::MAX_CXS_LEN, Some(9));
    assert_eq!(<u64 as ConstWriteXDR>::MAX_TXS_LEN, Some(8));
    assert_eq!(<i16 as ConstWriteXDR>::MAX_CXS_LEN, Some(3));
    assert_eq!(<bool as ConstWriteXDR>::MAX_CXS_LEN, Some(1));
    assert_eq!(<bool as ConstWriteXDR>::MAX_TXS_LEN, Some(4));
    assert_eq!(<f64 as ConstWriteXDR>::MAX_CXS_LEN, Some(8));
}

#[test]
fn composite_shapes_match_reference() {
    assert_trait_matches(&[0xdeu8, 0xad, 0xbe]); // [u8; 3] opaque fixed
    assert_trait_matches(&[1u32, 65536, 0]); // [u32; 3]
    assert_trait_matches(&Some(65536u32));
    assert_trait_matches(&Option::<u32>::None);
    assert_trait_matches(&Ok::<u32, i64>(7));
    assert_trait_matches(&Err::<u32, i64>(-9));
    assert_trait_matches(&(1u32, -1i64, true)); // tuple
    assert_trait_matches(&"hello"); // &str
    let s: &[u8] = &[9, 8, 7];
    assert_trait_matches(&s); // &[u8] variable opaque
    let t: &[u32] = &[1, 2, 3];
    assert_trait_matches(&t); // &[T]
}

#[test]
fn composite_max_values() {
    assert_eq!(<[u8; 5] as ConstWriteXDR>::MAX_CXS_LEN, Some(5));
    assert_eq!(<[u8; 5] as ConstWriteXDR>::MAX_TXS_LEN, Some(8)); // padded to 4
    assert_eq!(<[u32; 3] as ConstWriteXDR>::MAX_CXS_LEN, Some(15));
    assert_eq!(<Option<u64> as ConstWriteXDR>::MAX_CXS_LEN, Some(10));
    assert_eq!(<Option<u64> as ConstWriteXDR>::MAX_TXS_LEN, Some(12));
    assert_eq!(<Result<u32, i64> as ConstWriteXDR>::MAX_CXS_LEN, Some(10)); // 1 + max(5, 9)
    assert_eq!(<(u32, bool) as ConstWriteXDR>::MAX_CXS_LEN, Some(6));
    assert_eq!(<&str as ConstWriteXDR>::MAX_CXS_LEN, None);
    assert_eq!(<&[u8] as ConstWriteXDR>::MAX_CXS_LEN, None);
    assert_eq!(<() as ConstWriteXDR>::MAX_CXS_LEN, Some(0));
}

#[cfg(feature = "int128")]
#[test]
fn int128_trait_impls() {
    assert_trait_matches(&i128::MAX);
    assert_trait_matches(&u128::MAX);
    assert_eq!(<i128 as ConstWriteXDR>::MAX_CXS_LEN, Some(18));
    assert_eq!(<u128 as ConstWriteXDR>::MAX_TXS_LEN, Some(16));
}

#[cfg(feature = "alloc")]
#[test]
fn alloc_impls() {
    assert_eq!(<std::vec::Vec<u8> as ConstWriteXDR>::MAX_CXS_LEN, None);
    assert_eq!(<std::vec::Vec<u8> as ConstWriteXDR>::MAX_TXS_LEN, None);
    assert_eq!(<std::string::String as ConstWriteXDR>::MAX_CXS_LEN, None);
    assert_eq!(<std::string::String as ConstWriteXDR>::MAX_TXS_LEN, None);
    // byte equality for a small Vec<u32>
    let v: std::vec::Vec<u32> = std::vec![1u32, 2, 3];
    assert_trait_matches(&v);
}

#[cfg(feature = "alloc")]
mod xdr_common_types {
    use super::*;
    use xdrs::{BytesM, StringM, VecM};

    #[test]
    fn bounded_types_match_reference() {
        let b: BytesM<8> = vec![1u8, 2, 3].try_into().unwrap();
        assert_trait_matches(&b);
        let s: StringM<8> = "hi".try_into().unwrap();
        assert_trait_matches(&s);
        let v: VecM<u32, 4> = vec![1u32, 65536].try_into().unwrap();
        assert_trait_matches(&v);
    }

    #[test]
    fn bounded_max_values() {
        // CXS: len-varint(8)=1, so 1 + 8
        assert_eq!(<BytesM<8> as ConstWriteXDR>::MAX_CXS_LEN, Some(9));
        // TXS: 4 (len) + 8 (already 4-aligned)
        assert_eq!(<BytesM<8> as ConstWriteXDR>::MAX_TXS_LEN, Some(12));
        // TXS pads data: 4 + size_opaque_fixed(5) = 4 + 8
        assert_eq!(<BytesM<5> as ConstWriteXDR>::MAX_TXS_LEN, Some(12));
        // CXS: len-varint(4)=1 + 4 elems × 5
        assert_eq!(<VecM<u32, 4> as ConstWriteXDR>::MAX_CXS_LEN, Some(21));
        // len-varint(300) = 3 bytes (300 fits 2-byte payload + header)
        assert_eq!(<BytesM<300> as ConstWriteXDR>::MAX_CXS_LEN, Some(303));
        // Outer container at u32::MAX capacity × large element overflows → None
        assert_eq!(<VecM<VecM<u8>> as ConstWriteXDR>::MAX_CXS_LEN, None);
        // Finite nested bound composes: 1 (len varint for 4) + 4 × (5 + u32::MAX)
        assert_eq!(
            <VecM<VecM<u8>, 4> as ConstWriteXDR>::MAX_CXS_LEN,
            Some(17_179_869_201)
        );
        // Default VecM<u8> (MAX = u32::MAX)
        assert_eq!(
            <VecM<u8> as ConstWriteXDR>::MAX_CXS_LEN,
            Some(5 + u32::MAX as usize)
        );
    }
}

#[cfg(feature = "derive")]
mod derived {
    use super::*;
    use xdrs::{ConstWriteXDR, WriteXDR};

    #[derive(WriteXDR, ConstWriteXDR, Debug)]
    struct Point {
        x: i32,
        y: u64,
        tag: [u8; 3],
    }

    #[derive(WriteXDR, ConstWriteXDR, Debug)]
    #[repr(i32)]
    enum Shape {
        Circle { radius: u32 },
        Rect { w: u32, h: u32 },
    }

    #[test]
    fn derived_trait_matches_reference() {
        assert_trait_matches(&Point {
            x: -32,
            y: u64::MAX,
            tag: *b"abc",
        });
        assert_trait_matches(&Shape::Circle { radius: 65536 });
        assert_trait_matches(&Shape::Rect { w: 0, h: u32::MAX });
    }

    #[test]
    fn derived_max_values() {
        // 5 (i32) + 9 (u64) + 3 (opaque fixed)
        assert_eq!(<Point as ConstWriteXDR>::MAX_CXS_LEN, Some(17));
        // txs: 4 + 8 + 4 (3 padded)
        assert_eq!(<Point as ConstWriteXDR>::MAX_TXS_LEN, Some(16));
        // variants: disc(idx 0|1 -> 1 byte cxs) + payload; Rect wins: 1 + 5 + 5
        assert_eq!(<Shape as ConstWriteXDR>::MAX_CXS_LEN, Some(11));
        assert_eq!(<Shape as ConstWriteXDR>::MAX_TXS_LEN, Some(12)); // 4 + 4 + 4
    }

    #[cfg(feature = "alloc")]
    #[derive(WriteXDR, ConstWriteXDR, Debug)]
    #[xdrs(runtime_only)]
    struct WithString {
        name: xdrs::StringM<16>,
        id: u32,
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn runtime_only_derive() {
        let v = WithString {
            name: "abc".try_into().unwrap(),
            id: 7,
        };
        assert_trait_matches(&v);
        // 1 + 16 (StringM<16>) + 5 (u32)
        assert_eq!(<WithString as ConstWriteXDR>::MAX_CXS_LEN, Some(22));
    }

    // Exercises the enum runtime_only path: direct `match self` trait bodies.
    #[cfg(feature = "alloc")]
    #[derive(WriteXDR, ConstWriteXDR, Debug)]
    #[xdrs(runtime_only)]
    #[repr(i32)]
    enum Message {
        Ping,
        Text { body: xdrs::StringM<32> },
        Code { value: u32 },
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn runtime_only_enum_derive() {
        assert_trait_matches(&Message::Ping);
        assert_trait_matches(&Message::Text {
            body: "hi".try_into().unwrap(),
        });
        assert_trait_matches(&Message::Code { value: 65536 });
        // widest variant: disc(1) + StringM<32> CXS max (1 + 32 = 33) = 34
        assert_eq!(<Message as ConstWriteXDR>::MAX_CXS_LEN, Some(34));
    }
}
