use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;

use xdrs::*;

pub fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("simple 7", |b| b.iter(||
            simple_bench(black_box(7))
            ));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);

fn simple_bench(input_byte: u8) {

    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
    struct Boo<'a> {
        blerg: i32,
        bytes: &'a [u8; 9],
    }

    let bytes = [input_byte; 9];
    let val = Boo {
        blerg: -5,
        bytes: &bytes,
    };

    let encoded = b"\x3a\x07\x07\x07\x07\x07\x07\x07\x07\x07";
    let val2: Boo = serialize_cxs::decode(encoded).unwrap();

    assert_eq!(val2, val);
}
