use alloy_primitives::{Address, B256, U256};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use spacekit_quantum_verkle::commitment::{NistSisScheme, QuantumTree};

fn addr(byte: u8) -> Address {
    Address::from_slice(&[byte; 20])
}

fn key(byte: u8) -> B256 {
    B256::from_slice(&[byte; 32])
}

fn bench_set_get(c: &mut Criterion) {
    let mut group = c.benchmark_group("verkle_set_get");
    for size in [10u32, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                let mut tree = QuantumTree::<NistSisScheme>::new();
                for i in 0..size {
                    tree.set(&addr(1), &key(i as u8), U256::from(i as u64));
                }
                for i in 0..size {
                    let _ = tree.get(&addr(1), &key(i as u8));
                }
            });
        });
    }
    group.finish();
}

fn bench_proof(c: &mut Criterion) {
    let mut tree = QuantumTree::<NistSisScheme>::new();
    for i in 0..50u8 {
        tree.set(&addr(1), &key(i), U256::from(i as u64));
    }

    c.bench_function("verkle_create_proof", |b| {
        b.iter(|| {
            let _ = tree.create_proof(&addr(1), &key(25));
        });
    });
}

criterion_group!(benches, bench_set_get, bench_proof);
criterion_main!(benches);
