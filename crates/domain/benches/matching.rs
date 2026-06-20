//! Criterion bench — matching algorithm hot path (T-27).
//!
//! The matching algorithm runs on every order-create. It's CPU-
//! bound (no IO) and ships in `domain`, so we can bench it
//! without spinning up the API. Two scenarios:
//!
//!   1. `haversine_km` — the per-candidate distance calculation
//!   2. `top_n` over a realistic candidate pool size
//!
//! Run: `cargo bench -p kokkak-domain`

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use kokkak_domain::matching::{haversine_km, score, top_n, Candidate, Weights};
use uuid::Uuid;

fn random_candidates(n: usize) -> Vec<Candidate> {
    // Vientiane-ish lat/long band so distances look realistic.
    // Deterministic seed — criterion benches must be reproducible.
    (0..n)
        .map(|i| {
            // cheap LCG so the bench has no allocation overhead
            let x = ((i as u64 * 2654435761) % 1000) as f64 / 1000.0;
            let y = ((i as u64 * 40503 + 17) % 1000) as f64 / 1000.0;
            Candidate {
                id: Uuid::nil(),
                lat: 17.9 + x * 0.2,  // 17.9 .. 18.1
                lon: 102.5 + y * 0.3, // 102.5 .. 102.8
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
        // Vientiane to a point 30km north
        b.iter(|| haversine_km(17.97, 102.63, 18.27, 102.63))
    });
}

fn bench_top_n(c: &mut Criterion) {
    let mut group = c.benchmark_group("matching::top_n");
    let weights = default_weights();

    // 100 candidates is the typical "nearby technicians"
    // pool returned by the geo pre-filter. 1000 is the upper
    // bound during a wide-area search.
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
