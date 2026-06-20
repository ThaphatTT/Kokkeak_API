//! HTTP idempotency for safe POSTs (T-14).
//!
//! When a mobile client submits a `POST /orders` on a flaky
//! network, the retry can create a duplicate order — and on a
//! marketplace that means real money. The client sends an
//! `Idempotency-Key` header (a per-request unique string) and
//! the server replays the cached response on subsequent requests
//! with the same key (within the TTL).
//!
//! ## Semantics
//!
//! - Header: `Idempotency-Key: <unique-string>`. Required on
//!   protected POSTs (T-15 wires it to /orders, /payments,
//!   /auth/register). Optional everywhere else.
//! - Storage: scoped to the route. Different routes have
//!   independent keyspaces so `Idempotency-Key: X` on
//!   `POST /orders` does not collide with `X` on `POST /payments`.
//! - Cache hit: the exact cached response (status + headers
//!   plus body) is replayed with an `Idempotency-Replayed: true`
//!   response header so the client can verify the replay.
//! - Only `2xx` responses are cached. `4xx` and `5xx` are
//!   transparent to the cache so clients can retry safely on
//!   transient failures.
//! - TTL: configurable (default 24 hours, matches the IETF
//!   `Idempotency-Key` draft and Stripe's contract).
//!
//! ## Concurrent retries
//!
//! When two requests with the same key arrive simultaneously, the
//! "first writer wins" pattern applies: both pass the cache
//! check, both execute the handler, and the second `put()`
//! overwrites the first. This is acceptable for our handlers
//! because they are already idempotent at the DB level (the
//! NEW_DB schema enforces unique constraints on order and
//! payment references). A stricter "per-key lock" pattern can
//! be added later by changing the `InMemoryStore` to guard
//! `put` with a per-key `Mutex`.

use std::time::Duration;

/// Cached HTTP response, sufficient to replay the original
/// response byte-for-byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedResponse {
    /// HTTP status code (e.g. 200, 201, 422).
    pub status: u16,
    /// `Content-Type` header value (e.g. `application/json`).
    pub content_type: String,
    /// Response body bytes.
    pub body: Vec<u8>,
}

/// Port every HTTP idempotency store implements.
#[async_trait::async_trait]
pub trait IdempotencyStore: Send + Sync {
    /// Look up a cached response by key. Returns `None` on miss
    /// or on any backend error (we fail open — never block a
    /// request because the cache is sick).
    async fn get(&self, key: &str) -> Option<CachedResponse>;

    /// Store a response under the key. The TTL is the **maximum**
    /// time the entry will be honoured; the implementation is
    /// free to evict earlier (e.g. under memory pressure).
    async fn put(&self, key: &str, response: CachedResponse, ttl: Duration);

    /// Return the current number of cached entries. Diagnostic
    /// only — used by tests and `/metrics` style health checks.
    fn len(&self) -> usize;

    /// True when no entries are cached. Convenience for tests.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
