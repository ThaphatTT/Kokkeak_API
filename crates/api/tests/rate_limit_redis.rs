

use std::time::Duration;

use deadpool_redis::{Config, Runtime};
use kokkak_api::middleware::rate_limit_redis::{RateLimitDecision, RedisRateLimit};

fn live_url() -> Option<String> {
    std::env::var("KOKKAK_REDIS__TEST_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

fn build_limiter(burst: u64) -> RedisRateLimit {
    let url = live_url().expect("KOKKAK_REDIS__TEST_URL must be set");
    let cfg = Config::from_url(&url);
    let pool = cfg
        .create_pool(Some(Runtime::Tokio1))
        .expect("pool must build against live redis");
    RedisRateLimit::new(pool, burst, 1)
}

fn test_key(suffix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("kokkak:rl:test:{suffix}:{nanos}")
}

#[tokio::test]
async fn allows_requests_up_to_burst_then_denies() {
    let Some(_) = live_url() else {
        eprintln!("skipping: set KOKKAK_REDIS__TEST_URL to run");
        return;
    };
    let limiter = build_limiter(3);
    let key = test_key("upto_burst");

    for i in 1..=3 {
        let d = limiter.check(&key).await.expect("redis must respond");
        assert!(
            d.allowed,
            "hit #{i} should be allowed (burst=3, current={})",
            d.current
        );
        assert_eq!(d.current, i);
    }

    let d = limiter.check(&key).await.expect("redis must respond");
    assert!(!d.allowed, "hit #4 must be denied (current={})", d.current);
    assert_eq!(d.current, 4);

    assert!(
        d.retry_after_secs <= 1,
        "retry_after should reflect the 1s window, got {}",
        d.retry_after_secs
    );
}

#[tokio::test]
async fn counter_is_shared_across_instances() {

    let Some(_) = live_url() else {
        eprintln!("skipping: set KOKKAK_REDIS__TEST_URL to run");
        return;
    };
    let instance_a = build_limiter(5);
    let instance_b = build_limiter(5);
    let key = test_key("shared_counter");

    for _ in 0..3 {
        let d = instance_a.check(&key).await.expect("redis");
        assert!(d.allowed);
    }

    for i in 4..=5 {
        let d = instance_b.check(&key).await.expect("redis");
        assert!(d.allowed, "shared counter: hit #{i} should be allowed");
        assert_eq!(d.current, i);
    }

    let d = instance_a.check(&key).await.expect("redis");
    assert!(!d.allowed, "shared budget must be exhausted");
    assert_eq!(d.current, 6);
}

#[tokio::test]
async fn first_hit_sets_ttl_anchored_to_window_start() {

    let Some(_) = live_url() else {
        eprintln!("skipping: set KOKKAK_REDIS__TEST_URL to run");
        return;
    };
    let limiter = build_limiter(100);
    let key = test_key("ttl_anchor");

    let d1: RateLimitDecision = limiter.check(&key).await.expect("redis");
    assert!(d1.allowed);
    let ttl_after_first = d1.retry_after_secs;

    tokio::time::sleep(Duration::from_millis(1100)).await;

    let d2 = limiter.check(&key).await.expect("redis");
    assert!(d2.retry_after_secs <= 1);

    assert!(d2.current >= 1);

    let _ = ttl_after_first;
}
