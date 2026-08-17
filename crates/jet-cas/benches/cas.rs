//! CAS microbenchmarks.
//!
//! These exist to answer specific questions the plan's M2 gate depends on, not
//! to produce a general performance report:
//!
//! * how fast can we hash, i.e. what does ingest cost per byte?
//! * what does manifest encode/decode cost per entry? A 50k-file workspace
//!   touches this on every materialize, so a slow codec would show up as
//!   "cache hits are not actually fast".

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use jet_cas::tree::{EntryName, Node, Tree};
use jet_core::Digest;

fn bench_hash(c: &mut Criterion) {
    let mut g = c.benchmark_group("blake3");
    // 4 KiB is a typical source file; 1 MiB a typical compiled artifact.
    for size in [4usize << 10, 64 << 10, 1 << 20, 8 << 20] {
        let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
        g.throughput(Throughput::Bytes(size as u64));
        g.bench_with_input(BenchmarkId::from_parameter(size), &data, |b, d| {
            b.iter(|| black_box(Digest::of(black_box(d))));
        });
    }
    g.finish();
}

fn sample_tree(n: usize) -> Tree {
    let mut t = Tree::new();
    for i in 0..n {
        t.insert(
            EntryName::new(format!("file_{i:06}.rs")).unwrap(),
            Node::File {
                digest: Digest::of(&(i as u64).to_le_bytes()),
                size: 4096,
                executable: false,
            },
        );
    }
    t
}

fn bench_tree_codec(c: &mut Criterion) {
    let mut g = c.benchmark_group("tree");
    for n in [16usize, 256, 4096] {
        let t = sample_tree(n);
        let encoded = t.encode();

        g.throughput(Throughput::Elements(n as u64));
        g.bench_with_input(BenchmarkId::new("encode", n), &t, |b, t| {
            b.iter(|| black_box(black_box(t).encode()));
        });
        g.bench_with_input(BenchmarkId::new("decode", n), &encoded, |b, e| {
            b.iter(|| black_box(Tree::decode(black_box(e)).unwrap()));
        });
        g.bench_with_input(BenchmarkId::new("digest", n), &t, |b, t| {
            b.iter(|| black_box(black_box(t).digest()));
        });
    }
    g.finish();
}

criterion_group!(benches, bench_hash, bench_tree_codec);
criterion_main!(benches);
