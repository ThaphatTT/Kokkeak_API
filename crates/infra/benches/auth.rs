//! Criterion bench — JWT verify + argon2 verify (T-27).
//!
//! Both run on every authenticated request. Argon2 is expensive
//! by design (memory-hard), so the bench reports the per-call
//! cost — operators need that number to size CPU budgets.
//!
//! Run: `cargo bench -p kokkak-infra`

use std::time::Duration;

use criterion::{criterion_group, criterion_main, Criterion};
use kokkak_common::config::AuthSettings;
use kokkak_domain::Role;
use kokkak_infra::auth::jwt::JwtService;
use kokkak_infra::auth::password::PasswordHasherImpl;

fn make_jwt() -> (JwtService, String) {
    let settings = AuthSettings {
        jwt_secret: "bench-secret-do-not-use-in-prod-please-thanks".to_string(),
        issuer: "kokkak-bench".to_string(),
        access_ttl_secs: 900,
        refresh_ttl_secs: 2_592_000,
    };
    let svc = JwtService::new(&settings).expect("jwt service builds");
    let user_guid = uuid::Uuid::new_v4();
    let token = svc
        .issue_access(user_guid, &[Role::Customer], "USER_READ")
        .expect("token issued");
    (svc, token)
}

fn bench_jwt_verify(c: &mut Criterion) {
    let (svc, token) = make_jwt();
    c.bench_function("auth::jwt_verify", |b| b.iter(|| svc.verify(&token)));
}

fn bench_argon2_verify(c: &mut Criterion) {
    let hasher = PasswordHasherImpl::new();
    // Hash a known password once, then bench the verify path.
    // `hash` is much slower than `verify` (it has to choose
    // params + produce the encoded output), so measuring only
    // verify gives the realistic per-login cost.
    let hash = hasher
        .hash("correct horse battery staple")
        .expect("hash succeeds");

    let mut group = c.benchmark_group("auth::argon2_verify");
    // Argon2 is slow on purpose; let the bench run long enough
    // for stable numbers (default 5s is too short).
    group.measurement_time(Duration::from_secs(15));
    group.bench_function("verify_one", |b| {
        b.iter(|| hasher.verify("correct horse battery staple", &hash))
    });
    group.finish();
}

criterion_group!(benches, bench_jwt_verify, bench_argon2_verify);
criterion_main!(benches);
