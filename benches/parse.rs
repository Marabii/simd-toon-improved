#[macro_use]
extern crate criterion;

use core::time::Duration;

#[cfg(feature = "jemallocator")]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

use serde_json::Value;

use criterion::{BatchSize, Criterion, Throughput, criterion_group};
use simd_toon::Buffers;
use toon_format::decode_default;

use std::fs::File;
use std::io::Read;

fn to_borrowed_value(data: &mut [u8]) {
    simd_toon::to_borrowed_value(data).unwrap();
}

fn to_borrowed_value_with_buffers(data: &mut [u8], buffers: &mut Buffers) {
    simd_toon::to_borrowed_value_with_buffers(data, buffers).unwrap();
}

fn to_owned_value(data: &mut [u8]) -> simd_toon::OwnedValue {
    simd_toon::to_owned_value(data).unwrap()
}

fn toon_format_decode(data: &str) -> Value {
    decode_default(data).unwrap()
}

#[cfg(feature = "bench-serde")]
fn serde_from_slice(data: &[u8]) -> serde_json::Value {
    serde_json::from_slice(data).unwrap()
}

macro_rules! bench_file {
    ($name:ident) => {
        fn $name(c: &mut Criterion) {
            let core_ids = core_affinity::get_core_ids().unwrap();
            core_affinity::set_for_current(core_ids[0]);

            let mut vec = Vec::new();
            File::open(concat!("data/", stringify!($name), ".toon"))
                .unwrap()
                .read_to_end(&mut vec)
                .unwrap();

            let mut group = c.benchmark_group(stringify!($name));
            group.throughput(Throughput::Bytes(vec.len() as u64));
            group
                .warm_up_time(Duration::from_secs(1))
                .measurement_time(Duration::from_secs(20));

            let mut buffers = Buffers::default();

            group.bench_with_input("simd_toon::to_borrowed_value", &vec, |b, data| {
                b.iter_batched_ref(
                    || data.clone(),
                    |bytes| to_borrowed_value(bytes),
                    BatchSize::SmallInput,
                )
            });

            group.bench_with_input(
                "simd_toon::to_borrowed_value_with_buffers",
                &vec,
                |b, data| {
                    b.iter_batched_ref(
                        || data.clone(),
                        |bytes| to_borrowed_value_with_buffers(bytes, &mut buffers),
                        BatchSize::SmallInput,
                    )
                },
            );

            group.bench_with_input("simd_toon::to_owned_value", &vec, |b, data| {
                b.iter_batched_ref(
                    || data.clone(),
                    |bytes| to_owned_value(bytes),
                    BatchSize::SmallInput,
                )
            });

            group.bench_with_input("toon_format::decode_default", &vec, |b, data| {
                b.iter_with_large_drop(|| toon_format_decode(std::str::from_utf8(data).unwrap()))
            });

            #[cfg(feature = "bench-serde")]
            group.bench_with_input("serde_json::from_slice", &vec, |b, data| {
                b.iter_with_large_drop(|| serde_from_slice(data))
            });
        }
    };
}

bench_file!(apache_builds);
bench_file!(event_stacktrace_10kb);
bench_file!(github_events);
bench_file!(canada);
bench_file!(citm_catalog);
bench_file!(log);
bench_file!(twitter);

criterion_group!(
    benches,
    apache_builds,
    event_stacktrace_10kb,
    github_events,
    canada,
    citm_catalog,
    log,
    twitter
);
criterion_main!(benches);
