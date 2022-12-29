use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use rand::prelude::*;
use rand_xoshiro::Xoroshiro128PlusPlus;

use synthizer::fast_xoroshiro::*;

const LENGTHS: &[usize] = &[1usize, 4usize, 10usize, 16usize, 128usize];

fn wrapping_sum_black_box(slice: &[u64]) {
    let mut sum = 0u64;
    for i in slice {
        sum = sum.wrapping_add(*i);
    }
    black_box(sum);
}

pub fn rand_crate(c: &mut Criterion) {
    let mut group = c.benchmark_group("rand_xoroshiro");

    for output_len in LENGTHS.iter().copied() {
        group.throughput(Throughput::Elements(output_len as u64));
        group.bench_with_input(
            criterion::BenchmarkId::from_parameter(output_len),
            &(output_len,),
            |b, (output_len,)| {
                let mut buf = vec![0u64; *output_len];
                let mut gen = Xoroshiro128PlusPlus::seed_from_u64(5);
                b.iter(|| {
                    gen.fill(&mut buf[..]);
                    wrapping_sum_black_box(&buf[..]);
                });
            },
        );
    }
}

pub fn synthizer_fast(c: &mut Criterion) {
    let mut group = c.benchmark_group("FastXoroshiro128PlusPlus");

    for output_len in LENGTHS.iter().copied() {
        group.throughput(Throughput::Elements(output_len as u64));
        group.bench_with_input(
            criterion::BenchmarkId::from_parameter(output_len),
            &(output_len,),
            |b, (output_len,)| {
                let mut buf = vec![0u64; *output_len];
                let mut gen = FastXoroshiro128PlusPlus::<16>::new_seeded(5);
                b.iter(|| {
                    gen.gen_slice(&mut buf[..]);
                    wrapping_sum_black_box(&buf[..]);
                });
            },
        );
    }
}

criterion_group!(benches, rand_crate);
criterion_group!(benches2, synthizer_fast);
criterion_main!(benches, benches2);
