//! Auth adapters (M2).
//!
//! - `password`: argon2 hash + verify.
//! - `jwt`: HS256 issue / verify.
//! - `rate_limit`: in-memory per-(username, IP) login limiter.

pub mod jwt;
pub mod password;
pub mod rate_limit;
