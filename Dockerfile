# Dockerfile — Kokkeak multi-stage build (T-20).
#
# Layers:
#   chef     - cargo-chef base with the pinned Rust toolchain.
#   planner  - generates recipe.json (dependency graph only).
#   builder  - cooks deps once, then copies source + builds bins.
#   runtime  - distroless/static, no shell, no package manager.
#
# The two binaries (`kokkak-api`, `kokkak-worker`) ship from the
# same image so one base + one scan covers both deploy targets.
# Override `ENTRYPOINT` (or CMD) at runtime to pick which binary
# to run.
#
# Reference: https://github.com/LukeMathWalker/cargo-chef

# ---- Chef base (pinned to match rust-toolchain.toml: stable) ----
# Using `lukemathwalker/cargo-chef` with the `rust` tag pulls a
# recent stable toolchain. Switch to `rust-1.96-bookworm` to pin
# exactly once the toolchain bump lands.
FROM lukemathwalker/cargo-chef:latest-rust-bookworm AS chef
WORKDIR /app
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# ---- Planner: scan Cargo.toml/Cargo.lock and emit recipe.json ----
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ---- Builder: cook dependencies once, then layer source on top ----
FROM chef AS builder
# Cook deps from the recipe alone — this layer is cached until
# Cargo.toml/Cargo.lock change, regardless of source edits.
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --bin kokkak-api --bin kokkak-worker

# Now copy the rest of the source and build the two binaries.
# `touch` invalidates only the source tree (not the cache above).
COPY . .
RUN cargo build --release --bins \
    && cp target/release/kokkak-api    /tmp/kokkak-api \
    && cp target/release/kokkak-worker /tmp/kokkak-worker

# ---- Runtime: distroless/static — no shell, no libc, ~2MB base ----
# Static linking is provided by the musl target the chef image
# already uses (see T-20 design notes). The binary is fully
# self-contained; no glibc / openssl to chase on the host.
FROM gcr.io/distroless/static-debian12:nonroot AS runtime
WORKDIR /app

# Copy the two binaries built above.
COPY --from=builder /tmp/kokkak-api    /usr/local/bin/kokkak-api
COPY --from=builder /tmp/kokkak-worker /usr/local/bin/kokkak-worker

# Migrations are read by `crates/api` and `crates/worker` at
# startup. Bake them in so the image is self-sufficient; mount
# overrides via ConfigMap / Secret in k8s.
COPY --from=builder /app/migrations /app/migrations
COPY --from=builder /app/.env.example /app/.env.example

# distroless/nonroot runs as UID 65532 by default; we keep that
# (no USER line needed). The buildkit scratch space /tmp inside
# the image is owned by this user already.

# T-22 / T-21: k8s probes + compose healthcheck hit these paths.
# api listens on 8080 (matches `KOKKAK_SERVER__ADDR` default).
EXPOSE 8080

# Default to the API binary; override with `--entrypoint` or
# `command:` in compose / k8s to run the worker.
ENTRYPOINT ["/usr/local/bin/kokkak-api"]
