//! Infrastructure layer (เลเยอร์โครงสร้างพื้นฐาน).
//!
//! Concrete implementations of the repository / port traits defined in
//! `domain` (T07, T08, T09, M1.5, M2, M3, M8, M9), plus direct
//! infrastructure clients (T06, T09).
//!
//! See `AGENTS.md` § 3, 6, 7 for layering rules and the multi-database
//! topology (KOKKAK_MASTER, KOKKAK_CATALOG, KOKKAK_ORDER, ...).

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod auth;
pub mod cache;
pub mod db;
pub mod health;
pub mod pubsub;
pub mod queue;
pub mod storage;
