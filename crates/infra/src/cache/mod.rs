//! Cache clients + abstraction layer (T07 + T07A).
//!
//! - `redis`: thin deadpool-redis pool + `Cache` trait impl + pub/sub for
//!   cross-instance invalidation. The T07 deliverable.
//! - `layer`: two-tier cache (in-process `moka` L1 + `redis` L2) with
//!   `get_or_load`, TTL jitter, single-flight, negative caching, and
//!   pub/sub-driven L1 invalidation. The T07A deliverable.

pub mod layer;
pub mod redis;
