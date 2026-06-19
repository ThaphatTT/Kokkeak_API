//! Concrete `HealthCheck` adapters (ตัวตรวจสอบสถานะสำหรับ service ต่างๆ).
//!
//! Each submodule implements [`kokkak_domain::HealthCheck`] for a real
//! dependency. Construct the checks lazily inside `main` and register
//! them with the [`HealthRegistry`] (T05).
//!
//! Every adapter returns `Err(HealthError::Failed(_))` when the
//! underlying service is not configured (dev mode) so `/readyz`
//! reports it explicitly.

pub mod mongo;
pub mod nats;
pub mod redis;
pub mod sqlserver;
