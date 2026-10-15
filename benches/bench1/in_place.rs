//! Decoding into heap-allocated destinations.
//!
//! These measure the cost of building a value and then moving it into its
//! final allocation (`Box`, or a `Vec`/`VecM` slot), across a range of value
//! sizes, next to a plain by-value decode of the same input. Small values
//! should be unaffected by how they are placed; large ones expose the extra
//! stack frame and memcpy.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput};

use xdrs::{ReadXDROwned, VecM, WriteXDR, cxs, txs};

/// 16 bytes.
#[derive(Debug, Clone, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
pub struct Small {
    pub id: u64,
    pub amount: i32,
}

/// 256 bytes.
#[derive(Debug, Clone, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
pub struct Mid {
    pub id: u64,
    pub data: [u64; 31],
}

/// 4 KiB.
#[derive(Debug, Clone, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
pub struct Big4K {
    pub head: Mid,
    pub data: [u64; 480],
}

/// 64 KiB.
#[derive(Debug, Clone, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
pub struct Big64K {
    pub head: Big4K,
    pub data: [u64; 7680],
}

#[derive(xdrs::WriteXDR, xdrs::ReadXDR)]
struct TupleField {
    data: (u32, [u64; 8192]),
}

#[derive(xdrs::WriteXDR, xdrs::ReadXDR)]
struct OptionField {
    data: Option<[u64; 8192]>,
}

fn small() -> Small {
    Small { id: 7, amount: -3 }
}

fn mid() -> Mid {
    Mid {
        id: 1,
        data: core::array::from_fn(|i| i as u64),
    }
}

fn big4k() -> Big4K {
    Big4K {
        head: mid(),
        data: core::array::from_fn(|i| i as u64),
    }
}

fn big64k() -> Box<Big64K> {
    Box::new(Big64K {
        head: big4k(),
        data: core::array::from_fn(|i| i as u64),
    })
}

/// Benchmarks decoding `T` by value and into a `Box` with both formats.
fn bench_pointers<T>(c: &mut Criterion, name: &str, value: &T)
where
    T: WriteXDR + ReadXDROwned,
{
    let mut group = c.benchmark_group(format!("InPlace/{name}"));

    for (fmt, input) in [
        ("cxs", cxs::encode(value).unwrap()),
        ("txs", txs::encode(value).unwrap()),
    ] {
        let input: &[u8] = &input;
        group.throughput(Throughput::Bytes(input.len() as u64));

        let id = BenchmarkId::new("value_decode", fmt);
        group.bench_with_input(id, input, |b, input| {
            b.iter(|| {
                let out: T = match fmt {
                    "cxs" => cxs::decode(black_box(input)).unwrap(),
                    _ => txs::decode(black_box(input)).unwrap(),
                };
                black_box(&out);
            })
        });

        let id = BenchmarkId::new("Box_decode", fmt);
        group.bench_with_input(id, input, |b, input| {
            b.iter(|| {
                let out: Box<T> = match fmt {
                    "cxs" => cxs::decode(black_box(input)).unwrap(),
                    _ => txs::decode(black_box(input)).unwrap(),
                };
                black_box(&out);
            })
        });
    }

    group.finish();
}

pub fn bench_in_place_pointers(c: &mut Criterion) {
    bench_pointers(c, "Small16B", &small());
    bench_pointers(c, "Mid256B", &mid());
    bench_pointers(c, "Big4K", &big4k());
    bench_pointers(c, "Big64K", &*big64k());
    let data = core::array::from_fn(|i| i as u64);
    bench_pointers(c, "Tuple64K", &TupleField { data: (7, data) });
    bench_pointers(c, "OptionSome64K", &OptionField { data: Some(data) });
    bench_pointers(c, "OptionNone64K", &OptionField { data: None });
}

pub fn bench_in_place_vecm(c: &mut Criterion) {
    let mut group = c.benchmark_group("InPlace/Vec");
    const LEN: usize = 1024;

    macro_rules! bench_vec {
        ($ty:ty, $make:expr, $label:literal) => {{
            let value: VecM<$ty, { u32::MAX }> = VecM((0..LEN).map(|_| $make).collect());
            for (fmt, input) in [
                ("cxs", cxs::encode(&value).unwrap()),
                ("txs", txs::encode(&value).unwrap()),
            ] {
                let input: &[u8] = &input;
                group.throughput(Throughput::Bytes(input.len() as u64));

                let id =
                    BenchmarkId::new(concat!($label, "_Vec_decode"), format!("{fmt}/len={LEN}"));
                group.bench_with_input(id, input, |b, input| {
                    b.iter(|| {
                        let out: Vec<$ty> = match fmt {
                            "cxs" => cxs::decode(black_box(input)).unwrap(),
                            _ => txs::decode(black_box(input)).unwrap(),
                        };
                        black_box(&out);
                    })
                });

                let id =
                    BenchmarkId::new(concat!($label, "_VecM_decode"), format!("{fmt}/len={LEN}"));
                group.bench_with_input(id, input, |b, input| {
                    b.iter(|| {
                        let out: VecM<$ty, { u32::MAX }> = match fmt {
                            "cxs" => cxs::decode(black_box(input)).unwrap(),
                            _ => txs::decode(black_box(input)).unwrap(),
                        };
                        black_box(&out);
                    })
                });
            }
        }};
    }

    bench_vec!(Small, small(), "Small16B");
    bench_vec!(Mid, mid(), "Mid256B");
    bench_vec!(Big4K, big4k(), "Big4K");

    group.finish();
}
