//! Auth adapters (M2).
//!
//! - `password`: argon2 hash + verify.
//! - `jwt`: HS256 issue / verify.

pub mod jwt;
pub mod password;
