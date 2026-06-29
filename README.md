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


HOW TO DEPLOY
  WINDOWS
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File "scripts/publish-prod.ps1" -OutputZip

    .\scripts\publish-prod.ps1
