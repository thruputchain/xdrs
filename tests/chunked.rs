//! `ChunkedBytes` and `ChunkedVec` over borrowed chunks, which needs no
//! `alloc`: each must encode exactly like the slice of its concatenated
//! contents. tests/encodes_as.rs checks them against the owned wire types.

mod common;
use common::*;

use core::mem::MaybeUninit as MU;
use xdrs::{ChunkedBytes, ChunkedVec, WriteError, WriteXDR};

fn encoded<F: Format, T: WriteXDR + ?Sized>(value: &T) -> Vec<u8> {
    let mut buf = [MU::uninit(); 256];
    let written = F::encode_into(value, &mut buf).unwrap();
    // SAFETY: `encode_into` just initialized the first `written` bytes.
    unsafe { buf[..written].assume_init_ref() }.to_vec()
}

/// Every way of cutting `joined` into chunks, with an empty chunk at the
/// front and back of every other cutting.
fn chunkings<T>(joined: &[T]) -> Vec<Vec<&[T]>> {
    let cuts = joined.len().saturating_sub(1);
    (0..1u32 << cuts)
        .map(|mask| {
            let mut chunks = Vec::new();
            if mask % 2 == 1 {
                chunks.push(&joined[..0]);
            }
            let mut start = 0;
            for i in 0..cuts {
                if mask & (1 << i) != 0 {
                    chunks.push(&joined[start..=i]);
                    start = i + 1;
                }
            }
            chunks.push(&joined[start..]);
            if mask % 2 == 1 {
                chunks.push(&joined[joined.len()..]);
            }
            chunks
        })
        .collect()
}

// Lengths up to 9 cover every TXS padding amount, with chunks that end on
// and off a 4-byte boundary.
#[test]
fn chunked_bytes_encode_like_one_opaque() {
    let all: Vec<u8> = (1..=9).collect();
    for len in 0..=all.len() {
        let joined = &all[..len];
        let cxs = encoded::<CXS, _>(joined);
        let txs = encoded::<TXS, _>(joined);
        for chunks in chunkings(joined) {
            let chunked = ChunkedBytes::<&[&[u8]], 9>(&chunks);
            assert_eq!(chunked.len(), len);
            assert_eq!(chunked.is_empty(), len == 0);
            CXS::check_encode(&chunked, &cxs);
            TXS::check_encode(&chunked, &txs);
        }
    }
}

#[test]
fn chunked_bytes_pad_once_in_txs() {
    let chunks: [&[u8]; 3] = [b"a", b"bc", b"d"];
    let chunked = ChunkedBytes::<_>(&chunks[..]);

    TXS::check_encode(&chunked, &hex!("0000000461626364"));
    CXS::check_encode(&chunked, &hex!("0461626364"));

    let chunks: [&[u8]; 2] = [b"ab", b"c"];
    TXS::check_encode(&ChunkedBytes::<_>(&chunks[..]), &hex!("0000000361626300"));
}

#[test]
fn chunked_vec_encode_like_one_array() {
    let all: Vec<u32> = (1..=6).map(|n| n * 0x0101_0101).collect();
    for len in 0..=all.len() {
        let joined = &all[..len];
        let cxs = encoded::<CXS, _>(joined);
        let txs = encoded::<TXS, _>(joined);
        for chunks in chunkings(joined) {
            let chunked = ChunkedVec::<&[&[u32]], 6>(&chunks);
            assert_eq!(chunked.len(), len);
            assert_eq!(chunked.is_empty(), len == 0);
            CXS::check_encode(&chunked, &cxs);
            TXS::check_encode(&chunked, &txs);
        }
    }
}

fn encode_len<F: Format, T: WriteXDR>(value: &T) -> Result<usize, WriteError> {
    F::encode_into(value, &mut [MU::uninit(); 64])
}

// Like `BytesM` and `VecM`, and unlike a slice standing in for `VecM`, the
// bound is enforced when writing.
#[test]
fn chunked_write_rejects_more_than_max() {
    let bytes: [&[u8]; 2] = [b"abc", b"de"];
    assert!(encode_len::<TXS, _>(&ChunkedBytes::<_, 5>(&bytes[..])).is_ok());
    for result in [
        encode_len::<TXS, _>(&ChunkedBytes::<_, 4>(&bytes[..])),
        encode_len::<CXS, _>(&ChunkedBytes::<_, 4>(&bytes[..])),
    ] {
        assert!(matches!(result, Err(WriteError::InvalidArrayLength)));
    }

    let elements: [&[u32]; 2] = [&[1, 2], &[3]];
    assert!(encode_len::<TXS, _>(&ChunkedVec::<_, 3>(&elements[..])).is_ok());
    for result in [
        encode_len::<TXS, _>(&ChunkedVec::<_, 2>(&elements[..])),
        encode_len::<CXS, _>(&ChunkedVec::<_, 2>(&elements[..])),
    ] {
        assert!(matches!(result, Err(WriteError::InvalidArrayLength)));
    }
}
