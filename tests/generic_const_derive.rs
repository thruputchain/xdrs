#![cfg(feature = "derive")]
use core::mem::MaybeUninit;
use xdrs::{ConstWriteXDR, WriteXDR};

#[derive(WriteXDR, ConstWriteXDR, Debug)]
struct UserExn {
    code: u64,
    detail: [u8; 4],
}

// The sdk ContractExn<E> shape: generic payload enum.
// WriteXDR derive is verbatim-generics; user must state the WriteXDR bound.
// ConstWriteXDR derive synthesises E: ConstWriteXDR automatically.
#[derive(WriteXDR, ConstWriteXDR, Debug)]
#[repr(i32)]
enum ContractExn<E: xdrs::WriteXDR> {
    System { code: u32 },
    User { exn: E },
}

fn reference_cxs<T: WriteXDR>(v: &T) -> ([u8; 64], usize) {
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

// Generic code can bound on the trait — the whole point.
fn encode_infallible<'a, T: ConstWriteXDR>(v: &T, buf: &'a mut [u8]) -> &'a [u8] {
    let mut c = xdrs::cxs::Cursor::new(buf);
    v.write_cxs(&mut c);
    let n = c.pos();
    &buf[..n]
}

#[test]
fn generic_enum_derives_and_matches_reference() {
    let v = ContractExn::User {
        exn: UserExn {
            code: u64::MAX,
            detail: *b"oops",
        },
    };
    let (expected, len) = reference_cxs(&v);
    let mut buf = [0u8; 64];
    assert_eq!(encode_infallible(&v, &mut buf), &expected[..len]);
    assert_eq!(v.cxs_size(), len);

    let s = ContractExn::<UserExn>::System { code: 7 };
    let (expected, len) = reference_cxs(&s);
    let mut buf = [0u8; 64];
    assert_eq!(encode_infallible(&s, &mut buf), &expected[..len]);
}

#[test]
fn generic_enum_max_composes() {
    // System: 1 (disc) + 5 (u32 max); User: 1 (disc) + (9 + 4) = 14
    assert_eq!(
        <ContractExn<UserExn> as ConstWriteXDR>::MAX_CXS_LEN,
        Some(14)
    );
    // A huge-but-finite E composes: User arm = 1 (disc) + 5 (len varint) + u32::MAX.
    assert_eq!(
        <ContractExn<xdrs::VecM<u8>> as ConstWriteXDR>::MAX_CXS_LEN,
        Some(1 + 5 + u32::MAX as usize)
    );
    // A truly unbounded field type poisons the max but the type still derives.
    assert_eq!(<ContractExn<&[u8]> as ConstWriteXDR>::MAX_CXS_LEN, None);
}
