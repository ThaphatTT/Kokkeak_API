

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use kokkak_domain::matching::{haversine_km, score, top_n, Candidate, Weights};
use uuid::Uuid;

fn random_candidates(n: usize) -> Vec<Candidate> {

    (0..n)
        .map(|i| {

            let x = ((i as u64 * 2654435761) % 1000) as f64 / 1000.0;
            let y = ((i as u64 * 40503 + 17) % 1000) as f64 / 1000.0;
            Candidate {
                id: Uuid::nil(),
                lat: 17.9 + x * 0.2,
                lon: 102.5 + y * 0.3,
                rating: 3.0 + x * 2.0,
                level: (i % 5) as i32 + 1,
                current_load: (i % 3) as i32,
                max_load: 5,
            }
        })
        .collect()
}

fn default_weights() -> Weights {
    Weights {
        distance: 1.0,
        rating: 0.5,
        level: 0.2,
        load: 0.3,
    }
}

fn bench_haversine(c: &mut Criterion) {
    c.bench_function("matching::haversine_km", |b| {

        b.iter(|| haversine_km(17.97, 102.63, 18.27, 102.63))
    });
}

fn bench_top_n(c: &mut Criterion) {
    let mut group = c.benchmark_group("matching::top_n");
    let weights = default_weights();

    for n in [100_usize, 1_000].iter() {
        let pool = random_candidates(*n);
        group.bench_with_input(BenchmarkId::from_parameter(n), n, |b, _| {
            b.iter(|| top_n(17.97, 102.63, &pool, 10, &weights))
        });
    }
    group.finish();
}

fn bench_score(c: &mut Criterion) {
    let mut group = c.benchmark_group("matching::score");
    let weights = default_weights();
    let candidates = random_candidates(1_000);
    group.bench_function("score_one", |b| {
        let c0 = &candidates[0];
        b.iter(|| score(17.97, 102.63, c0, &weights))
    });
    group.finish();
}

criterion_group!(benches, bench_haversine, bench_top_n, bench_score);
criterion_main!(benches);
