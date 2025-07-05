#![feature(maybe_uninit_array_assume_init)]
#![feature(maybe_uninit_uninit_array_transpose)]
#![feature(maybe_uninit_slice)]

use criterion::{criterion_group, criterion_main, Criterion};
// use std::hint::black_box;

use xdrs::*;
use xdrs::serialize_cxs::{encode, encode_into, UninitBuf};

pub fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("boo encode", |b| b.iter(||
            boo_encode()
            ));
    c.bench_function("boo decode", |b| b.iter(||
            boo_decode()
            ));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);

#[derive(Debug, Copy, Clone, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
struct Boo<'a> {
    blerg: i32,
    blorg: u128,
    bilig: u64,
    bulba: &'a [u8],
    bytes: &'a [u8; 9],
}

const N_BOOS: usize = 1; // 50;
const BOOS: [Boo; N_BOOS] = [const { Boo {
    blerg: -5,
    blorg: 0xdeadbeefdeadbeefdeadbeefdeadbeef,
    bilig: 0xcafebabecafebabe,
    bulba: &BOO_BYTES,
    bytes: &BOO_BYTES,
} }; N_BOOS];
const BOO_BYTES: [u8; 9] = [7; 9];
const BUFLEN: usize = 2500;

const FOOS: [Boo; N_BOOS] = [const { Boo {
    blerg: -7,
    blorg: 0xfeedbeeffeedbeeffeedbeeffeedbeef,
    bilig: 0xbabecafebabecafe,
    bulba: &BOO_BYTES,
    bytes: &BOO_BYTES,
} }; N_BOOS];

const CODEC_ITERATIONS: usize = 1000;

fn boo_encode() {
    // A buffer to receive the encoded value
    let mut buf = UninitBuf::<BUFLEN>::new();

    // Encode the value many times
    for _ in 0..CODEC_ITERATIONS {
        buf.init(|buf| encode_into(&BOOS, buf)).unwrap();
    }

    // run `cargo test --benches` to test with the asserts
    #[cfg(test)]
    {
        // Extract the now-initialized portion of the buffer
        let encoded = unsafe {
            buf.assume_init()
        };

        let boos2: [Boo; N_BOOS] =
            serialize_cxs::decode(encoded).unwrap();
        debug_assert_eq!(&boos2, &BOOS);
    }
}

fn boo_decode()
{
    let encoded_owned = encode(&BOOS).unwrap();
    let encoded = encoded_owned.as_ref();

    let mut boos2 = FOOS;
    for _ in 0..CODEC_ITERATIONS {
        boos2 = serialize_cxs::decode(encoded).unwrap();
    }

    // run `cargo test --benches` to test with the asserts
    debug_assert_eq!(&boos2, &BOOS);
}
