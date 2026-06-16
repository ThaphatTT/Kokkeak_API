//! Kokkeak Worker — NATS consumer for background jobs (M4).
//!
//! Reads from the subjects defined in AGENTS.md § 10:
//!
//! | subject           | handler              | external side-effect |
//! |-------------------|----------------------|----------------------|
//! | `noti.push`       | `noti_push`          | FCM (stubbed)        |
//! | `comm.email`      | `comm_email`         | SMTP (stubbed)       |
//! | `chat.persist`    | `chat_persist`       | MongoDB              |
//! | `order.dispatch`  | `order_dispatch`     | broadcast candidates |
//! | `points.recalc`   | `points_recalc`      | DB (stubbed)         |
//!
//! Every handler is **idempotent** (AGENTS.md § 10) — the message id
//! is checked in the `Idempotency` cache before any side-effect runs.

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod handlers;
pub mod idempotency;
pub mod runner;

pub use idempotency::{Idempotency, IdempotencyKey, InMemoryIdempotency, RedisIdempotency};
pub use runner::{Worker, WorkerConfig};
