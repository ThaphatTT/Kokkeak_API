# Kokkeak API

Handyman / technician marketplace backend (Laos) — **Rust + axum + tiberius**.

> **Project rules live in `AGENTS.md`** which is intentionally NOT
> committed (per project convention). Refer to your local copy for
> coding standards, dependency rules, and the full build plan.

## Build

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

## Run (T01 — minimal)

```bash
cargo run --bin kokkak-api
# → http://0.0.0.0:3000/healthz returns 200 OK
# → http://0.0.0.0:3000/readyz  returns 200 (with empty checks list)
# → http://0.0.0.0:3000/metrics returns Prometheus text
```

## M1 Status

| Task | Status | Notes |
|------|--------|-------|
| T06 SQL Server | ⏸ STUB | `tiberius` + `bb8-tiberius` deferred to M1.5 — see `crates/infra/src/db/mssql.rs` |
| T07 Redis | ✅ real impl | `deadpool-redis` + `redis` crate, `Cache` trait impl. Pub/sub invalidation listener returns empty stream until M1.5 |
| T07A Caching layer | ✅ real impl | `moka` L1 + optional Redis L2; `get_or_load`, TTL cap, hit/miss metrics. Single-flight + cross-instance invalidation listener deferred to M1.5 |
| T08 NATS | ✅ real impl | `async-nats` + JetStream context, `QueuePort` trait impl. Liveness via `flush()` with 2s timeout |
| T09 MongoDB | ✅ real impl | `mongodb` 3.x driver + collection/ping helpers. Migration runner (file discovery + sort) is unit-tested; SQL apply is stubbed until tiberius lands |

**51 tests pass** (4 api + 24 common + 14 domain + 8 infra + 1 doc-test).
**clippy clean** (-D warnings), **rustfmt clean**.

## Workspace Layout

```
kokkak_api/
├── crates/
│   ├── common/       # error, config, telemetry, response envelope
│   ├── domain/       # entities, business rules, traits (no IO)
│   ├── application/  # use cases
│   ├── infra/        # tiberius, mongo, redis, nats, fcm, s3 adapters
│   ├── api/          # axum HTTP server
│   └── worker/       # NATS consumer
├── migrations/       # .sql versioned (SQL Server)
├── tests/            # integration tests
├── Cargo.toml        # workspace manifest
├── rust-toolchain.toml
└── .gitignore
```

## Tech Stack (locked — see AGENTS.md § 2)

| Layer | Crate |
|-------|-------|
| Web | `axum` + `tower` + `tower-http` |
| Async | `tokio` |
| SQL Server | `tiberius` + `bb8-tiberius` |
| MongoDB | `mongodb` |
| Cache | `fred` or `deadpool-redis` |
| Queue | `async-nats` |
| Errors | `thiserror` (lib), `anyhow` (bin) |
| Auth | `jsonwebtoken` + `argon2` |
| Decimal | `rust_decimal` (NO f64 for money) |

## Build Order (see `KOKKAK_MIGRATION_PLAN/06_BUILD_PLAN_FOR_AI.md`)

| M | Status | Tasks |
|---|--------|-------|
| M0 Foundation | ✅ done | T01–T05 |
| M1 Infra connect | 🚧 scaffolded | T06 (stub, M1.5), T07, T07A, T08, T09 |
| M2 Auth & RBAC | ⏳ | T10–T13 |
| M3 Shared domain | ⏳ | T14–T17 |
| M4 Queue & Worker | ⏳ | T18–T20 |
| M5–M11 | ⏳ | T21–T44 |
