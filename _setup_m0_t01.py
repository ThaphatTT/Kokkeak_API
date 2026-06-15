"""Create M0 T01 file structure: workspace + 6 crates + api /healthz."""

from pathlib import Path

ROOT = Path(r"C:\Users\crybo\Desktop\Develop\Kokkeak_API")
CRATES = ROOT / "crates"

# ---------- 1. Create directory structure ----------
for c in ["common", "domain", "application", "infra", "api", "worker"]:
    (CRATES / c / "src").mkdir(parents=True, exist_ok=True)
(ROOT / "migrations").mkdir(exist_ok=True)
print("Created directory structure")

# ---------- 2. Cargo.toml for each lib crate ----------
for c in ["common", "domain", "application", "infra"]:
    role = {
        "common": "Common utilities: error, config, telemetry, response envelope",
        "domain": "Domain entities, value objects, business rules, traits (no framework/DB)",
        "application": "Use cases: orchestrate domain + repo traits (1 action = 1 use case)",
        "infra": "Adapters: tiberius (SQL Server), mongodb, redis, nats, fcm, S3",
    }[c]
    toml = f"""[package]
name = "kokkak-{c}"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
description = "{role}"

[dependencies]
# Filled in per task — see AGENTS.md § 3
"""
    (CRATES / c / "Cargo.toml").write_text(toml, encoding="utf-8")
    print(f"  crates/{c}/Cargo.toml")

# ---------- 3. Cargo.toml for api (binary) ----------
(CRATES / "api" / "Cargo.toml").write_text(
    """[package]
name = "kokkak-api"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
description = "HTTP API server (axum) — entry point for web/admin/mobile clients"

[[bin]]
name = "kokkak-api"
path = "src/main.rs"

[dependencies]
# Core
tokio = { workspace = true }
axum = { workspace = true }
""",
    encoding="utf-8",
)
print("  crates/api/Cargo.toml")

# ---------- 4. Cargo.toml for worker (binary) ----------
(CRATES / "worker" / "Cargo.toml").write_text(
    """[package]
name = "kokkak-worker"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
description = "NATS consumer — background jobs (push, email, pdf, points, etc.)"

[[bin]]
name = "kokkak-worker"
path = "src/main.rs"

[dependencies]
# Filled in M4 (T18)
""",
    encoding="utf-8",
)
print("  crates/worker/Cargo.toml")

# ---------- 5. lib.rs for each lib crate ----------
lib_content = {
    "common": """//! Common layer
//!
//! Houses shared infrastructure used by every other crate:
//! error types, configuration loader, telemetry, response envelope,
//! and small utilities (UUID v7, time, decimal).
//!
//! See AGENTS.md § 3, 11, 12, 14 for the standards this layer enforces.

#![deny(unsafe_code)]
#![warn(missing_docs)]
""",
    "domain": """//! Domain layer
//!
//! Pure Rust: entities, value objects, business rules, and repository
//! **traits** (ports).
//!
//! **Dependency rule** (AGENTS.md § 6): this crate MUST NOT import
//! anything from the framework or DB world (no `axum`, no `tiberius`,
//! no `mongodb`). All IO is expressed through traits in `domain::traits`.

#![deny(unsafe_code)]
#![warn(missing_docs)]
""",
    "application": """//! Application layer
//!
//! Use cases: each public function orchestrates one business action
//! (e.g. `create_order`, `login`, `approve_technician`).
//!
//! Depends on `domain` for entities/traits and on `infra` only
//! through `Arc<dyn Trait>` (constructor-injected).

#![deny(unsafe_code)]
#![warn(missing_docs)]
""",
    "infra": """//! Infra layer
//!
//! Concrete implementations of the repository traits defined in `domain`:
//! SQL Server via `tiberius`, MongoDB, Redis, NATS, FCM, S3, etc.
//!
//! Also houses the SQL migration runner (since `sqlx-cli` does not
//! support MSSQL).

#![deny(unsafe_code)]
#![warn(missing_docs)]
""",
}
for c, content in lib_content.items():
    (CRATES / c / "src" / "lib.rs").write_text(content, encoding="utf-8")
    print(f"  crates/{c}/src/lib.rs")

# ---------- 6. main.rs for api with /healthz (T01 acceptance) ----------
(CRATES / "api" / "src" / "main.rs").write_text(
    """//! Kokkeak API entry point.
//!
//! T01 (M0): minimal — only `GET /healthz` returns 200 OK.
//! Full HTTP routing, middleware, and graceful shutdown are added in T05+.

use axum::{Router, http::StatusCode, response::IntoResponse, routing::get};

#[tokio::main]
async fn main() {
    let app = Router::new().route("/healthz", get(healthz));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind 0.0.0.0:3000");

    println!(
        "kokkak-api listening on http://{}",
        listener.local_addr().expect("local_addr")
    );

    axum::serve(listener, app)
        .await
        .expect("server error");
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}
""",
    encoding="utf-8",
)
print("  crates/api/src/main.rs")

# ---------- 7. main.rs for worker (placeholder, M4) ----------
(CRATES / "worker" / "src" / "main.rs").write_text(
    """//! Kokkeak Worker entry point.
//!
//! Placeholder — full implementation lands in M4 (T18) when the
//! NATS consumer skeleton is built. See AGENTS.md § 4 build order.

fn main() {
    println!("kokkak-worker placeholder — see AGENTS.md § 4 (M4 / T18)");
}
""",
    encoding="utf-8",
)
print("  crates/worker/src/main.rs")

# ---------- 8. README.md (root) ----------
(ROOT / "README.md").write_text(
    """# Kokkeak API

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
""",
    encoding="utf-8",
)
print("  README.md")

# ---------- 9. .env.example ----------
(ROOT / ".env.example").write_text(
    """# Kokkeak API — environment variables
# Copy to .env (NEVER commit) and adjust values per environment.

# ---- Server ----
KOKKAK_SERVER__ADDR=0.0.0.0:3000
KOKKAK_SERVER__WORKERS=4

# ---- Logging (T03) ----
# Format: json (prod) | pretty (dev)
KOKKAK_LOG__FORMAT=pretty
RUST_LOG=info,kokkak_api=debug

# ---- Filled in by T02+ ----
# KOKKAK_DB__SQLSERVER_URL=sqlserver://user:pass@host:1433/KOKKAK_MASTER
# KOKKAK_DB__MONGO_URL=mongodb://user:pass@host:27017/kokkak
# KOKKAK_REDIS__URL=redis://host:6379
# KOKKAK_NATS__URL=nats://host:4222
# KOKKAK_JWT__SECRET=change-me-32-bytes-min
# KOKKAK_S3__BUCKET=kokkak-uploads
# KOKKAK_S3__ENDPOINT=https://s3.example.com
""",
    encoding="utf-8",
)
print("  .env.example")

# ---------- 10. migrations/.gitkeep (keep dir in git) ----------
(ROOT / "migrations" / ".gitkeep").write_text("", encoding="utf-8")
print("  migrations/.gitkeep")

print("\n=== Final tree ===")
for p in sorted(ROOT.rglob("*")):
    if p.is_file() and "target" not in str(p):
        rel = p.relative_to(ROOT)
        print(f"  {rel}")
