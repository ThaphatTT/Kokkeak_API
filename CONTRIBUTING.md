# Contributing to Kokkeak_API

> Public-facing version of the team's engineering rules.
> The internal `AGENTS.md` has the long-form discussion;
> this document is the actionable checklist for contributors.

Kokkeak is the backend for the KOKKAK handyman/technician
marketplace. The stack is **Rust 2021 + axum 0.7 + tokio +
tiberius (SQL Server) + mongodb + redis + nats**. Architecture
follows the hexagonal pattern: `domain` → `application` →
`infra` / `api` / `worker`, with `common` for cross-cutting
concerns.

---

## 1. Workflow

1. **Branch from `main`.** Use one branch per task:
   `chore/<task-id>-<slug>`, `feat/<task-id>-<slug>`,
   `fix/<task-id>-<slug>`, `refactor/<scope>`, `docs/<scope>`.
2. **Keep PRs small.** One task = one PR. If you find work
   outside the scope, file an issue rather than bundling it.
3. **Open a draft PR early.** CI runs the four gates listed in
   §3 on every push; a draft PR surfaces problems before the
   review starts.
4. **Self-review before requesting review.** Re-read the diff
   in the GitHub UI — formatting, secrets, debug `println!`,
   and unrelated edits show up at a glance there.
5. **Use the project's PR template** (auto-populated). Fill in
   the **Why** / **What** / **How to verify** sections.

## 2. Local setup

```bash
# Toolchain is pinned in `rust-toolchain.toml`; rustup picks
# it up automatically.
rustup component add rustfmt clippy

# Copy the example env file and fill in dev-only values.
cp .env.example .env

# Run the four CI gates locally before pushing.
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features -- --test-threads=1
cargo build --workspace --release
```

The `--test-threads=1` flag is required because `rust_i18n`
uses a global locale (see `AGENTS.md` §13).

## 3. The four CI gates

Every PR must pass:

| # | Gate | What it catches |
|---|---|---|
| 1 | `cargo fmt --check` | Style drift across the team. |
| 2 | `cargo clippy -- -D warnings` | Anti-patterns, perf hits, correctness slips. |
| 3 | `cargo test --workspace` | Behavioural regressions. |
| 4 | `cargo build --release` | Production-mode compile errors. |

CI runs on `ubuntu-latest` (primary); Windows is a dev mirror
only. Don't rely on Windows-only behaviour to pass the suite.

## 4. Coding rules (summary)

The long version lives in `AGENTS.md`. Highlights:

- **Layering:** `domain` must not import `axum`, `tiberius`,
  `serde-db`, or anything IO-related. `application` calls
  `domain` traits. `infra` implements the traits. `api` /
  `worker` call use cases.
- **Money:** use `rust_decimal::Decimal`. Never `f64`.
  See `AGENTS.md` §7.5.
- **SQL:** always bound parameters (`@P1`, `@P2`, ...).
  No `format!()` into SQL.
- **Errors:** `thiserror` in libraries, `anyhow` only in
  binaries / workers. Map to `AppError` at the handler edge.
- **Auth:** argon2 for password hashing. JWT secret read from
  env, never hardcoded.
- **i18n:** every user-visible string lives in
  `crates/common/locales/{th,en,lo}.yml`. No hardcoded UI
  strings.

### Forbidden patterns

These will fail review (and clippy / fmt where automatable):

- `.unwrap()` / `.expect()` in runtime paths.
- Plain-text passwords (use `argon2` via
  `crates/infra/src/auth/password.rs`).
- String-interpolated SQL.
- Emoji in `tracing` / `log` output.
- Hardcoded URLs, secrets, or environment-specific config.

## 5. Testing

- Unit tests live at the bottom of the file they test
  (`#[cfg(test)] mod tests`).
- Integration tests live in `tests/` at the crate root.
- DB-touching tests use `testcontainers` — never point at a
  shared dev database.
- Name tests after the behaviour:
  `fn create_order_returns_conflict_when_idempotency_key_exists`.

Coverage targets: `domain` 90%+, `application` 85%+,
`infra` 70%+, `api` 60%+.

## 6. Commits

Follow the project commit style (`AGENTS.md` §16.1):

- Subject ≤ 50 chars, imperative mood, no trailing period.
- Wrap the body at 72 chars.
- Body only when it adds context the subject can't.
- Reference the task ID when one exists (`T-19: enable ALPN h2`).

## 7. Definition of Done

A feature is done when **all** of these are true:

- [ ] Lives in the right layer (no `axum` in `domain`, etc.).
- [ ] Request DTO has `#[derive(Validate)]` + `.validate()`.
- [ ] Errors map to `AppError` → envelope response.
- [ ] Tracing log + metrics for the happy and error paths.
- [ ] Heavy / external work offloaded to the worker (NATS).
- [ ] Read-heavy reads cached (per `AGENTS.md` §9.3 groups).
- [ ] i18n keys added in `th` / `en` / `lo` (if any UI string).
- [ ] Tests cover the new behaviour.
- [ ] `cargo fmt`, `cargo clippy`, `cargo test`, `cargo build`
      all pass.
- [ ] `README.md` / `.env.example` updated if config changed.

## 8. Branch protection (what GitHub enforces)

The `main` branch is **protected**. As a contributor you will
notice:

- **Direct pushes to `main` are blocked.** Always go through
  a PR.
- **CI must be green** before merge is allowed.
- **At least one review approval** is required. CODEOWNERS
  auto-assigns the right reviewer by file path.
- **Linear history** is preferred — squash or rebase before
  merge; no merge commits into `main`.
- **Force pushes** to `main` are blocked.

These are configured under **Settings → Branches → Branch
protection rules → `main`** in the GitHub repo UI. The
platform team maintains the rule; if a rule blocks a legitimate
flow, ping `@kokkak/platform`.

## 9. Where to ask

- **Code review / design questions** → CODEOWNERS for the
  file you're touching (see `.github/CODEOWNERS`).
- **CI / deploy / infra** → `#kokkak-platform` (or
  `@kokkak/platform`).
- **Security issue** → email `security@kokkak.example` rather
  than opening a public issue.
- **Architecture decision** → open an ADR under
  `docs/adr/0001-<title>.md` following the template in
  `AGENTS.md` §4.

## 10. License

Proprietary. By contributing you agree that your contributions
are licensed under the project's existing terms. Do not import
GPL/AGPL dependencies without checking with the platform team
first — `cargo-deny` (CI gate) will fail the build anyway.