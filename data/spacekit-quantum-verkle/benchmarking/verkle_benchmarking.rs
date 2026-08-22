use criterion::{black_box, criterion_group, criterion_main, Criterion};
use spacekit_quantum_verkle::new_quantum_tree;

fn bench_verkle_operations(c: &mut Criterion) {
    let mut tree = new_quantum_tree();
    c.bench_function("quantum_tree_set_get", |b| {
        b.iter(|| {
            let key = black_box(alloy_primitives::B256::from([0u8; 32]));
            let address = black_box(alloy_primitives::Address::from([0u8; 20]));
            tree.set(&address, &key, alloy_primitives::U256::from(1u64));
            let _ = tree.get(&address, &key);
        })
    });
}

criterion_group!(benches, bench_verkle_operations);
criterion_main!(benches);