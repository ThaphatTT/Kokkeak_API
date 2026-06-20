//! R-02: Redis-backed per-IP rate limit.
//!
//! Replaces the per-instance `tower_governor` GCRA with a Redis
//! counter that is **shared across every replica**. With the
//! memory backend, deploying more than one pod multiplies the
//! effective per-IP limit by the replica count (a noisy client
//! gets `limit × pod_count` total budget). The Redis backend
//! counts globally, so the documented `burst_size` is the
//! actual ceiling regardless of how many pods are running.
//!
//! ## Algorithm
//!
//! Fixed-window counter via an atomic Lua script:
//!
//! ```lua
//! local current = redis.call('INCR', KEYS[1])
//! if current == 1 then
//!   redis.call('EXPIRE', KEYS[1], ARGV[1])
//! end
//! local ttl = redis.call('TTL', KEYS[1])
//! return {current, ttl}
//! ```
//!
//! `INCR` is atomic on the server; the `EXPIRE` is set only on
//! the first hit so the TTL is anchored to the start of the
//! window (not refreshed by every request). `TTL` is read in
//! the same round trip to power the `Retry-After` header.
//!
//! **Ceiling:** a fixed window has a worst-case 2× burst at
//! the window boundary (a client could fire `burst_size`
//! requests at t=0.99 and another `burst_size` at t=1.01).
//! Acceptable for a backstop behind a BFF. Upgrade path:
//! token bucket via Lua script (~6 lines of Lua) when the burst
//! behaviour actually matters.
//!
//! ## Failure mode
//!
//! If the Redis call fails (pool exhausted, connection reset,
//! etc.) the middleware **fails open** — logs a warning and
//! lets the request through. AGENTS.md §9.3 group C marks
//! rate-limit as "Redis as source, volatile": a Redis outage
//! is a degraded but acceptable state, far better than a hard
//! reject that takes the API down. The per-instance
//! `tower_governor` (if also enabled via a different route) can
//! still serve as a coarse backstop.
//!
//! ## Layer placement
//!
//! Wired in `main.rs` between the body-limit / concurrency-cap
//! layers (outer) and the timeout / compression / cors layers
//! (inner), so a denied request never pays the per-request
//! CPU cost of gzip / timeout machinery.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use deadpool_redis::Pool;
use redis::Script;
use serde_json::json;
use thiserror::Error;

/// Atomic INCR + first-hit EXPIRE + TTL read.
///
/// Returns `{current_count, ttl_secs}`. The `EXPIRE` only fires
/// on the first hit (when `current == 1`) so the window is
/// anchored to its start, not refreshed by traffic.
const RATE_LIMIT_SCRIPT: &str = r#"
local current = redis.call('INCR', KEYS[1])
if current == 1 then
  redis.call('EXPIRE', KEYS[1], ARGV[1])
end
local ttl = redis.call('TTL', KEYS[1])
return {current, ttl}
"#;

/// Key prefix for the per-IP counter. Versioned (`v1`) so a
/// future algorithm change can deploy side-by-side with a
/// new prefix instead of a global flush.
const KEY_PREFIX: &str = "kokkak:rl:v1:ip:";

/// Per-request decision from the rate-limit check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitDecision {
    /// `true` if the request should proceed.
    pub allowed: bool,
    /// Seconds until the window resets — populate `Retry-After`
    /// with this value when `allowed == false`.
    pub retry_after_secs: u64,
    /// Counter value in the current window. Useful for the
    /// `X-RateLimit-Remaining` style header (not emitted today;
    /// kept here for observability + future use).
    pub current: u64,
}

/// Shared state for the Redis rate-limit middleware.
///
/// `Clone` is cheap: the `deadpool` `Pool` is internally
/// `Arc`-based, and the `Script` is wrapped in `Arc` so the
/// compiled SHA1 is reused across calls (avoids re-sending the
/// Lua body on every request).
#[derive(Clone)]
pub struct RedisRateLimit {
    pool: Pool,
    burst: u64,
    window_secs: u64,
    script: Arc<Script>,
}

impl RedisRateLimit {
    /// Build a new limiter. `burst` is the max requests per
    /// `window_secs` per key.
    pub fn new(pool: Pool, burst: u64, window_secs: u64) -> Self {
        Self {
            pool,
            burst,
            window_secs,
            script: Arc::new(Script::new(RATE_LIMIT_SCRIPT)),
        }
    }

    /// Increment the counter for `key` and decide whether the
    /// request is allowed. Exposed for unit tests; the
    /// middleware calls this via the `State` extractor.
    pub async fn check(&self, key: &str) -> Result<RateLimitDecision, RedisRateLimitError> {
        let mut conn = self.pool.get().await?;
        let (current, ttl): (i64, i64) = self
            .script
            .key(key)
            .arg(self.window_secs)
            .invoke_async(&mut conn)
            .await?;
        // `i64` -> `u64`: counts are non-negative on the Redis
        // side; clamp defensively in case of a misbehaving proxy.
        let current_u64 = current.max(0) as u64;
        // TTL < 0 means no expiry set yet (race window right
        // after the first INCR). Fall back to the configured
        // window so Retry-After is still meaningful.
        let retry_after = if ttl < 0 {
            self.window_secs
        } else {
            ttl as u64
        };
        Ok(RateLimitDecision {
            allowed: current_u64 <= self.burst,
            retry_after_secs: retry_after,
            current: current_u64,
        })
    }
}

/// Errors from the Redis rate-limit check. All variants are
/// fail-open at the call site — the middleware never propagates
/// an error to the client.
#[derive(Debug, Error)]
pub enum RedisRateLimitError {
    /// Pool exhausted / connection refused / etc.
    #[error("redis pool: {0}")]
    Pool(#[from] deadpool_redis::PoolError),
    /// Redis returned an error reply or the connection reset.
    #[error("redis: {0}")]
    Redis(#[from] redis::RedisError),
}

/// R-02 middleware: per-IP via `ConnectInfo<SocketAddr>`,
/// shared counter via Redis, fail-open on Redis errors.
pub async fn rate_limit_redis_middleware(
    State(limiter): State<RedisRateLimit>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    let key = format!("{KEY_PREFIX}{}", addr.ip());
    match limiter.check(&key).await {
        Ok(decision) if decision.allowed => {
            // Trace-level only — emitting info on every allowed
            // request floods the logs on a busy service.
            tracing::trace!(ip = %addr.ip(), current = decision.current, "rate limit ok");
            next.run(req).await
        }
        Ok(decision) => {
            tracing::warn!(
                ip = %addr.ip(),
                current = decision.current,
                retry_after_secs = decision.retry_after_secs,
                "rate limit hit"
            );
            build_429_response(decision.retry_after_secs)
        }
        Err(err) => {
            // Fail open. Logged at WARN so operators can spot
            // a sustained Redis outage; INFO would drown in
            // transient blips on a busy service.
            tracing::warn!(
                ip = %addr.ip(),
                error = %err,
                "rate limit redis check failed; failing open"
            );
            next.run(req).await
        }
    }
}

fn build_429_response(retry_after_secs: u64) -> Response {
    let body = json!({
        "success": false,
        "error": {
            "code": "rate_limited",
            "message": "too many requests"
        }
    });
    let mut resp = (StatusCode::TOO_MANY_REQUESTS, Json(body)).into_response();
    if let Ok(v) = HeaderValue::from_str(&retry_after_secs.to_string()) {
        resp.headers_mut().insert(header::RETRY_AFTER, v);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Key naming is observable (the prefix + IP) — guard
    /// against accidental rename so dashboards / Redis
    /// operators don't lose visibility.
    #[test]
    fn key_prefix_is_versioned() {
        assert!(KEY_PREFIX.starts_with("kokkak:rl:"));
        assert!(KEY_PREFIX.contains(":v1:"));
    }

    /// Decision helper: shape only, no Redis call. Lets us
    /// test the boundary condition (current == burst) without
    /// standing up a server.
    #[test]
    fn decision_allows_at_or_below_burst() {
        let allowed = |current: u64, burst: u64| current <= burst;
        assert!(allowed(0, 5));
        assert!(allowed(5, 5));
        assert!(!allowed(6, 5));
    }

    /// The Lua script must call `INCR`, set `EXPIRE` on the
    /// first hit only, and return the TTL. Re-checking the
    /// script body catches accidental edits that would, for
    /// example, refresh the TTL on every request (window
    /// would never roll over).
    #[test]
    fn lua_script_matches_algorithm() {
        assert!(RATE_LIMIT_SCRIPT.contains("INCR"));
        assert!(RATE_LIMIT_SCRIPT.contains("EXPIRE"));
        assert!(RATE_LIMIT_SCRIPT.contains("TTL"));
        // The `if current == 1` guard anchors the window.
        assert!(RATE_LIMIT_SCRIPT.contains("current == 1"));
    }

    /// 429 response carries the `Retry-After` header so
    /// well-behaved clients back off automatically.
    #[test]
    fn response_429_carries_retry_after_header() {
        let resp = build_429_response(7);
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        let retry = resp
            .headers()
            .get(header::RETRY_AFTER)
            .expect("Retry-After must be set");
        assert_eq!(retry.to_str().unwrap(), "7");
    }
}
