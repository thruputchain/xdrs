mod uninit_buf;
use uninit_buf::*;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

const N_CODEC_ITERATIONS: usize = 100;
const N_LARGE_DECODE_ITERATIONS: usize = 10;

use xdrs::{VecM, cxs, txs};

fn bench_codec(c: &mut Criterion) {
    let mut group = c.benchmark_group("Codec");

    // Make inputs for decoding below
    let cxs_input_vec = cxs::encode(&VALUES).unwrap();
    let cxs_input: &[u8] = cxs_input_vec.as_ref();
    let cxs_size = cxs_input.len();
    group.throughput(Throughput::Bytes((N_CODEC_ITERATIONS * cxs_size) as u64));

    let id = BenchmarkId::new(
        "encode_cxs",
        format!("output {}B * {}", cxs_size, N_CODEC_ITERATIONS),
    );
    group.bench_with_input(id, &VALUES, |b, values| b.iter(|| run_encode_cxs(values)));

    let id = BenchmarkId::new(
        "decode_cxs",
        format!("input {}B * {}", cxs_size, N_CODEC_ITERATIONS),
    );
    group.bench_with_input(id, cxs_input, |b, input| b.iter(|| run_decode_cxs(input)));

    let txs_input_vec = txs::encode(&VALUES).unwrap();
    let txs_input: &[u8] = txs_input_vec.as_ref();
    let txs_size = txs_input.len();
    group.throughput(Throughput::Bytes((N_CODEC_ITERATIONS * txs_size) as u64));

    let id = BenchmarkId::new(
        "encode_txs",
        format!("output {}B * {}", txs_size, N_CODEC_ITERATIONS),
    );
    group.bench_with_input(id, &VALUES, |b, values| b.iter(|| run_encode_txs(values)));

    let id = BenchmarkId::new(
        "decode_txs",
        format!("input {}B * {}", txs_size, N_CODEC_ITERATIONS),
    );
    group.bench_with_input(id, txs_input, |b, input| b.iter(|| run_decode_txs(input)));

    group.finish();
}

fn bench_variable_opaque_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("VariableOpaqueDecode");

    for &len in &[255usize, 256, 257] {
        let payload = vec![0x5au8; len];

        let cxs_input_vec = cxs::encode(payload.as_slice()).unwrap();
        let cxs_input: &[u8] = cxs_input_vec.as_ref();
        group.throughput(Throughput::Bytes(
            (N_CODEC_ITERATIONS * cxs_input.len()) as u64,
        ));

        let id = BenchmarkId::new("decode_cxs", format!("len={len}"));
        group.bench_with_input(id, cxs_input, |b, input| {
            b.iter(|| run_decode_bytes_cxs(input, len))
        });

        let txs_input_vec = txs::encode(payload.as_slice()).unwrap();
        let txs_input: &[u8] = txs_input_vec.as_ref();
        group.throughput(Throughput::Bytes(
            (N_CODEC_ITERATIONS * txs_input.len()) as u64,
        ));

        let id = BenchmarkId::new("decode_txs", format!("len={len}"));
        group.bench_with_input(id, txs_input, |b, input| {
            b.iter(|| run_decode_bytes_txs(input, len))
        });
    }

    group.finish();
}

fn bench_large_vecm_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("LargeVecMDecode");

    let value = VecM::<u8, LARGE_VECM_MAX>(vec![0xa5; LARGE_VECM_LEN]);

    let cxs_input_vec = cxs::encode(&value).unwrap();
    let cxs_input: &[u8] = cxs_input_vec.as_ref();
    group.throughput(Throughput::Bytes(
        (N_LARGE_DECODE_ITERATIONS * cxs_input.len()) as u64,
    ));
    let id = BenchmarkId::new(
        "decode_cxs",
        format!(
            "payload={}B * {}",
            LARGE_VECM_LEN, N_LARGE_DECODE_ITERATIONS
        ),
    );
    group.bench_with_input(id, cxs_input, |b, input| {
        b.iter(|| run_decode_large_vecm_cxs(input))
    });

    let txs_input_vec = txs::encode(&value).unwrap();
    let txs_input: &[u8] = txs_input_vec.as_ref();
    group.throughput(Throughput::Bytes(
        (N_LARGE_DECODE_ITERATIONS * txs_input.len()) as u64,
    ));
    let id = BenchmarkId::new(
        "decode_txs",
        format!(
            "payload={}B * {}",
            LARGE_VECM_LEN, N_LARGE_DECODE_ITERATIONS
        ),
    );
    group.bench_with_input(id, txs_input, |b, input| {
        b.iter(|| run_decode_large_vecm_txs(input))
    });

    group.finish();
}

fn bench_vecm_struct_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("VecMStructDecode");

    for &len in &[64usize, 512, 4096, 8192] {
        let value: VecM<Entry, { u32::MAX }> = VecM(
            (0..len)
                .map(|i| Entry {
                    id: i as u64,
                    amount: i as i32 * 7,
                })
                .collect(),
        );

        let txs_input_vec = txs::encode(&value).unwrap();
        let txs_input: &[u8] = txs_input_vec.as_ref();
        group.throughput(Throughput::Bytes(
            (N_LARGE_DECODE_ITERATIONS * txs_input.len()) as u64,
        ));

        let id = BenchmarkId::new("decode_txs", format!("len={len}"));
        group.bench_with_input(id, txs_input, |b, input| {
            b.iter(|| {
                let mut output: VecM<Entry, { u32::MAX }>;
                output = txs::decode(input).unwrap();
                for _ in 1..N_LARGE_DECODE_ITERATIONS {
                    output = txs::decode(input).unwrap();
                }
                debug_assert_eq!(output.0.len(), len);
            })
        });

        let cxs_input_vec = cxs::encode(&value).unwrap();
        let cxs_input: &[u8] = cxs_input_vec.as_ref();
        group.throughput(Throughput::Bytes(
            (N_LARGE_DECODE_ITERATIONS * cxs_input.len()) as u64,
        ));

        let id = BenchmarkId::new("decode_cxs", format!("len={len}"));
        group.bench_with_input(id, cxs_input, |b, input| {
            b.iter(|| {
                let mut output: VecM<Entry, { u32::MAX }>;
                output = cxs::decode(input).unwrap();
                for _ in 1..N_LARGE_DECODE_ITERATIONS {
                    output = cxs::decode(input).unwrap();
                }
                debug_assert_eq!(output.0.len(), len);
            })
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_codec,
    bench_variable_opaque_decode,
    bench_large_vecm_decode,
    bench_vecm_struct_decode
);
criterion_main!(benches);

fn run_encode_cxs(values: &[Value; N_VALUES]) {
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
        let encoded = unsafe { buf.assume_init() };

        let values2: [Value; N_VALUES] = cxs::decode(encoded).unwrap();
        debug_assert_eq!(&values2, &VALUES);
    }
}

fn run_encode_txs(values: &[Value; N_VALUES]) {
    // A buffer to receive the encoded value
    const BUFLEN: usize = 3000;
    let mut buf = UninitBuf::<BUFLEN>::new();

    // Encode the value many times
    for _ in 0..N_CODEC_ITERATIONS {
        buf.init(|buf| txs::encode_into(values, buf)).unwrap();
    }

    // run `cargo test --benches` to test with the asserts
    #[cfg(test)]
    {
        // Extract the now-initialized portion of the buffer
        let encoded = unsafe { buf.assume_init() };

        let values2: [Value; N_VALUES] = txs::decode(encoded).unwrap();
        debug_assert_eq!(&values2, &VALUES);
    }
}

fn run_decode_cxs(input: &[u8]) {
    let mut output: [Value; N_VALUES];

    output = cxs::decode(input).unwrap();
    for _ in 1..N_CODEC_ITERATIONS {
        output = cxs::decode(input).unwrap();
    }

    // run `cargo test --benches` to test with the asserts
    debug_assert_eq!(output, VALUES);
}

fn run_decode_txs(input: &[u8]) {
    let mut output: [Value; N_VALUES];

    output = txs::decode(input).unwrap();
    for _ in 1..N_CODEC_ITERATIONS {
        output = txs::decode(input).unwrap();
    }

    // run `cargo test --benches` to test with the asserts
    debug_assert_eq!(output, VALUES);
}

fn run_decode_bytes_cxs(input: &[u8], expected_len: usize) {
    let mut output: &[u8];

    output = cxs::decode(input).unwrap();
    for _ in 1..N_CODEC_ITERATIONS {
        output = cxs::decode(input).unwrap();
    }

    debug_assert_eq!(output.len(), expected_len);
}

fn run_decode_bytes_txs(input: &[u8], expected_len: usize) {
    let mut output: &[u8];

    output = txs::decode(input).unwrap();
    for _ in 1..N_CODEC_ITERATIONS {
        output = txs::decode(input).unwrap();
    }

    debug_assert_eq!(output.len(), expected_len);
}

fn run_decode_large_vecm_cxs(input: &[u8]) {
    type LargeVecM = VecM<u8, LARGE_VECM_MAX>;

    let mut output: LargeVecM;

    output = cxs::decode(input).unwrap();
    for _ in 1..N_LARGE_DECODE_ITERATIONS {
        output = cxs::decode(input).unwrap();
    }

    debug_assert_eq!(output.0.len(), LARGE_VECM_LEN);
}

fn run_decode_large_vecm_txs(input: &[u8]) {
    type LargeVecM = VecM<u8, LARGE_VECM_MAX>;

    let mut output: LargeVecM;

    output = txs::decode(input).unwrap();
    for _ in 1..N_LARGE_DECODE_ITERATIONS {
        output = txs::decode(input).unwrap();
    }

    debug_assert_eq!(output.0.len(), LARGE_VECM_LEN);
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
const LARGE_VECM_MAX: u32 = 1_048_576;
const LARGE_VECM_LEN: usize = 262_144;

const VALUES: [Value; N_VALUES] = [const {
    Value {
        blerg: -5,
        blorg: 0xdeadbeefdeadbeefdeadbeefdeadbeef,
        bilig: 0xcafebabecafebabe,
        bulba: &VALUE_BYTES,
        bytes: &VALUE_BYTES,
    }
}; N_VALUES];

const VALUE_BYTES: [u8; 9] = [7; 9];

#[derive(Debug, Copy, Clone, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
struct Entry {
    id: u64,
    amount: i32,
}

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
