mod uninit_buf;
use uninit_buf::*;

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

use xdrs::{VecM, cxs, txs};

fn bench_codec(c: &mut Criterion) {
    let mut group = c.benchmark_group("Codec");

    let cxs_input_vec = cxs::encode(&VALUES).unwrap();
    let cxs_input: &[u8] = cxs_input_vec.as_ref();
    let cxs_size = cxs_input.len();
    group.throughput(Throughput::Bytes(cxs_size as u64));

    let id = BenchmarkId::new("encode_cxs", format!("output {cxs_size}B"));
    group.bench_with_input(id, &VALUES, |b, values| {
        const BUFLEN: usize = 2500;
        let mut buf = UninitBuf::<BUFLEN>::new();
        b.iter(|| buf.init(|buf| cxs::encode_into(black_box(values), buf)).unwrap());
    });

    let id = BenchmarkId::new("decode_cxs", format!("input {cxs_size}B"));
    group.bench_with_input(id, cxs_input, |b, input| {
        b.iter(|| {
            let output: [Value; N_VALUES] = cxs::decode(black_box(input)).unwrap();
            black_box(output);
        })
    });

    let txs_input_vec = txs::encode(&VALUES).unwrap();
    let txs_input: &[u8] = txs_input_vec.as_ref();
    let txs_size = txs_input.len();
    group.throughput(Throughput::Bytes(txs_size as u64));

    let id = BenchmarkId::new("encode_txs", format!("output {txs_size}B"));
    group.bench_with_input(id, &VALUES, |b, values| {
        const BUFLEN: usize = 3000;
        let mut buf = UninitBuf::<BUFLEN>::new();
        b.iter(|| buf.init(|buf| txs::encode_into(black_box(values), buf)).unwrap());
    });

    let id = BenchmarkId::new("decode_txs", format!("input {txs_size}B"));
    group.bench_with_input(id, txs_input, |b, input| {
        b.iter(|| {
            let output: [Value; N_VALUES] = txs::decode(black_box(input)).unwrap();
            black_box(output);
        })
    });

    group.finish();
}

fn bench_large_vecm_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("LargeVecMDecode");

    type LargeVecM = VecM<u8, LARGE_VECM_MAX>;

    let value: LargeVecM = VecM(vec![0xa5; LARGE_VECM_LEN]);

    let cxs_input_vec = cxs::encode(&value).unwrap();
    let cxs_input: &[u8] = cxs_input_vec.as_ref();
    group.throughput(Throughput::Bytes(cxs_input.len() as u64));
    let id = BenchmarkId::new("decode_cxs", format!("payload={}B", LARGE_VECM_LEN));
    group.bench_with_input(id, cxs_input, |b, input| {
        b.iter(|| {
            let output: LargeVecM = cxs::decode(black_box(input)).unwrap();
            black_box(&output);
        })
    });

    let txs_input_vec = txs::encode(&value).unwrap();
    let txs_input: &[u8] = txs_input_vec.as_ref();
    group.throughput(Throughput::Bytes(txs_input.len() as u64));
    let id = BenchmarkId::new("decode_txs", format!("payload={}B", LARGE_VECM_LEN));
    group.bench_with_input(id, txs_input, |b, input| {
        b.iter(|| {
            let output: LargeVecM = txs::decode(black_box(input)).unwrap();
            black_box(&output);
        })
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
        group.throughput(Throughput::Bytes(txs_input.len() as u64));

        let id = BenchmarkId::new("decode_txs", format!("len={len}"));
        group.bench_with_input(id, txs_input, |b, input| {
            b.iter(|| {
                let output: VecM<Entry, { u32::MAX }> =
                    txs::decode(black_box(input)).unwrap();
                black_box(&output);
            })
        });

        let cxs_input_vec = cxs::encode(&value).unwrap();
        let cxs_input: &[u8] = cxs_input_vec.as_ref();
        group.throughput(Throughput::Bytes(cxs_input.len() as u64));

        let id = BenchmarkId::new("decode_cxs", format!("len={len}"));
        group.bench_with_input(id, cxs_input, |b, input| {
            b.iter(|| {
                let output: VecM<Entry, { u32::MAX }> =
                    cxs::decode(black_box(input)).unwrap();
                black_box(&output);
            })
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_codec,
    bench_large_vecm_decode,
    bench_vecm_struct_decode
);
criterion_main!(benches);

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
