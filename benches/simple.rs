#![feature(maybe_uninit_array_assume_init)]
#![feature(maybe_uninit_uninit_array_transpose)]
#![feature(maybe_uninit_slice)]

use criterion::{
    criterion_group,
    criterion_main,
    Criterion,
    BenchmarkId,
    Throughput,
};
// use std::hint::black_box;

const N_CODEC_ITERATIONS: usize = 100;

use xdrs::*;
use xdrs::serialize_cxs as cxs;
use xdrs::serialize_cxs::UninitBuf;

fn bench_codec(c: &mut Criterion) {
    let mut group = c.benchmark_group("Codec");

    // Make input for decoding below
    let input_vec = cxs::encode(&VALUES).unwrap();
    let input: &[u8] = input_vec.as_ref();
    let size = input.len();
    group.throughput(Throughput::Bytes((N_CODEC_ITERATIONS * size) as u64));

    let id = BenchmarkId::new("encode", format!("Output {}B * {}", size, N_CODEC_ITERATIONS));
    group.bench_with_input(id, &VALUES, |b, values|
        b.iter(|| run_encode(values)));

    let id = BenchmarkId::new("decode", format!("Input {}B * {}", size, N_CODEC_ITERATIONS));
    group.bench_with_input(id, input, |b, input|
        b.iter(|| run_decode(input)));

    group.finish();
}

criterion_group!(benches, bench_codec);
criterion_main!(benches);

fn run_encode(values: &[Value; N_VALUES]) {

    // A buffer to receive the encoded value
    const BUFLEN: usize = 2500;
    let mut buf = UninitBuf::<BUFLEN>::new();

    // Encode the value many times
    for _ in 0..N_CODEC_ITERATIONS {
        buf.init(|buf| cxs::encode_into(values, buf)).unwrap();
    }

    // run `cargo test --benches` to test with the asserts
    #[cfg(test)]
    {
        // Extract the now-initialized portion of the buffer
        let encoded = unsafe {
            buf.assume_init()
        };

        let values2: [Value; N_VALUES] =
            serialize_cxs::decode(encoded).unwrap();
        debug_assert_eq!(&values2, &VALUES);
    }
}

fn run_decode<'a>(input: &[u8])
{
    let mut output: [Value; N_VALUES];

    output = cxs::decode(input).unwrap();
    for _ in 1..N_CODEC_ITERATIONS {
        output = cxs::decode(input).unwrap();
    }

    // run `cargo test --benches` to test with the asserts
    debug_assert_eq!(output, VALUES);
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
struct Value<'a> {
    blerg: i32,
    blorg: u128,
    bilig: u64,
    bulba: &'a [u8],
    bytes: &'a [u8; 9],
}

const N_VALUES: usize = 50;

const VALUES: [Value; N_VALUES] = [const { Value {
    blerg: -5,
    blorg: 0xdeadbeefdeadbeefdeadbeefdeadbeef,
    bilig: 0xcafebabecafebabe,
    bulba: &VALUE_BYTES,
    bytes: &VALUE_BYTES,
} }; N_VALUES];

const VALUE_BYTES: [u8; 9] = [7; 9];

/*
const fn default_value() -> Value<'static> {
    Value {
        blerg: 0,
        blorg: 0,
        bilig: 0,
        bulba: &ZEROS,
        bytes: &ZEROS,
    }
}
const ZEROS: [u8; 9] = [0; 9];
*/
