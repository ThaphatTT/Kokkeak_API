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
```

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
| M0 Foundation | 🚧 in progress | T01–T05 |
| M1 Infra connect | ⏳ | T06–T09 + T07A |
| M2 Auth & RBAC | ⏳ | T10–T13 |
| M3 Shared domain | ⏳ | T14–T17 |
| M4 Queue & Worker | ⏳ | T18–T20 |
| M5–M11 | ⏳ | T21–T44 |
