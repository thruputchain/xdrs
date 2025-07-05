#![feature(maybe_uninit_array_assume_init)]

use criterion::{criterion_group, criterion_main, Criterion};
// use std::hint::black_box;
use core::mem::MaybeUninit as MU;

use xdrs::*;
use xdrs::serialize_cxs::encode_into;

pub fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("simple 7", |b| b.iter(||
            simple_bench()
            ));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);

#[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
struct Boo<'a> {
    blerg: i32,
    bytes: &'a [u8; 9],
}

fn simple_bench() {

    const BOO_BYTES: [u8; 9] = [7; 9];
    const N_BOOS: usize = 20;
    const BOOS: [Boo; N_BOOS] = [const { Boo {
        blerg: -5,
        bytes: &BOO_BYTES,
    } }; N_BOOS];

    const BUFLEN: usize = 200;
    let mut buf: [MU<u8>; BUFLEN] = [MU::uninit(); BUFLEN];

    let mut written = 0;
    for _ in 0..1000 {
        written = encode_into(&BOOS, &mut buf);
    }
    let buf = unsafe { MU::array_assume_init(buf) };
    let encoded = &buf[..written];

    let mut boos2: [Boo; N_BOOS] = BOOS;
    for _ in 0..1000 {
        boos2 = serialize_cxs::decode(encoded).unwrap();
    }
    debug_assert_eq!(&boos2, &BOOS);
}
