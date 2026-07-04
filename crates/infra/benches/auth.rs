

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

    let hash = hasher
        .hash("correct horse battery staple")
        .expect("hash succeeds");

    let mut group = c.benchmark_group("auth::argon2_verify");

    group.measurement_time(Duration::from_secs(15));
    group.bench_function("verify_one", |b| {
        b.iter(|| hasher.verify("correct horse battery staple", &hash))
    });
    group.finish();
}

criterion_group!(benches, bench_jwt_verify, bench_argon2_verify);
criterion_main!(benches);
