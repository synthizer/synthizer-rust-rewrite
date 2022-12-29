use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};

use synthizer::convolution::convolve_direct;
use synthizer::views::*;

pub fn criterion_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("direct_convolution");

    for output_len in [256usize, 512usize, 1024usize, 1 << 20] {
        for impulse_len in [32usize, 128usize, 512usize, 1024usize, 8192usize] {
            for channels in [1usize, 2] {
                group.throughput(Throughput::Elements(output_len as u64));
                group.bench_with_input(
                    criterion::BenchmarkId::from_parameter(format!(
                        "output_len={output_len}, channels={channels}, impulse_len={impulse_len}"
                    )),
                    &(output_len, channels, impulse_len),
                    |b, (output_len, channels, impulse_len)| {
                        let input_frames = output_len + impulse_len - 1;
                        let input = (0..input_frames * *channels)
                            .map(|i| i as f32)
                            .collect::<Vec<_>>();
                        let mut output: Vec<f32> = vec![0.0f32; *output_len * *channels];
                        let impulse = (0..*impulse_len).map(|i| i as f32).collect::<Vec<_>>();
                        for ch in 0..*channels {
                            b.iter(|| {
                                convolve_direct(
                                    &ImmutableSliceView::new(&input[..], *channels),
                                    ch,
                                    &mut MutableSliceView::<_, false>::new(
                                        &mut output[..],
                                        *channels,
                                    ),
                                    ch,
                                    &impulse,
                                );
                                black_box(output.last());
                            });
                        }
                    },
                );
            }
        }
    }
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
