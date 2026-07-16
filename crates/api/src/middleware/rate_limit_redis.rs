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

const RATE_LIMIT_SCRIPT: &str = r#"
local current = redis.call('INCR', KEYS[1])
if current == 1 then
  redis.call('EXPIRE', KEYS[1], ARGV[1])
end
local ttl = redis.call('TTL', KEYS[1])
return {current, ttl}
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitDecision {
    pub allowed: bool,

    pub retry_after_secs: u64,

    pub current: u64,
}

#[derive(Clone)]
pub struct RedisRateLimit {
    pool: Pool,
    burst: u64,
    window_secs: u64,
    script: Arc<Script>,
    key_prefix: String,
}

impl RedisRateLimit {
    pub fn new(pool: Pool, burst: u64, window_secs: u64, namespace: &str) -> Self {
        Self {
            pool,
            burst,
            window_secs,
            script: Arc::new(Script::new(RATE_LIMIT_SCRIPT)),
            key_prefix: format!("{}:kokkeak:rl:v1:ip:", namespace),
        }
    }

    pub fn key_prefix(&self) -> &str {
        &self.key_prefix
    }

    pub async fn check(&self, key: &str) -> Result<RateLimitDecision, RedisRateLimitError> {
        let mut conn = self.pool.get().await?;
        let (current, ttl): (i64, i64) = self
            .script
            .key(key)
            .arg(self.window_secs)
            .invoke_async(&mut conn)
            .await?;

        let current_u64 = current.max(0) as u64;

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

#[derive(Debug, Error)]
pub enum RedisRateLimitError {
    #[error("redis pool: {0}")]
    Pool(#[from] deadpool_redis::PoolError),

    #[error("redis: {0}")]
    Redis(#[from] redis::RedisError),
}

pub async fn rate_limit_redis_middleware(
    State(limiter): State<RedisRateLimit>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    let key = format!("{}{}", limiter.key_prefix(), addr.ip());
    match limiter.check(&key).await {
        Ok(decision) if decision.allowed => {
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

    #[test]
    fn decision_allows_at_or_below_burst() {
        let allowed = |current: u64, burst: u64| current <= burst;
        assert!(allowed(0, 5));
        assert!(allowed(5, 5));
        assert!(!allowed(6, 5));
    }

    #[test]
    fn lua_script_matches_algorithm() {
        assert!(RATE_LIMIT_SCRIPT.contains("INCR"));
        assert!(RATE_LIMIT_SCRIPT.contains("EXPIRE"));
        assert!(RATE_LIMIT_SCRIPT.contains("TTL"));

        assert!(RATE_LIMIT_SCRIPT.contains("current == 1"));
    }

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
