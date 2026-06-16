//! Object-storage adapters (M9).
//!
//! Two adapters satisfy [`kokkak_domain::Storage`]:
//!
//! - [`memory::MemoryStorage`] — in-process `HashMap`. Used in
//!   dev / tests; no presigned URLs (returns `None`).
//! - [`s3::S3Storage`] — S3 / S3-compatible (MinIO) via
//!   `rust-s3`. Presigned `GetObject` URLs are generated
//!   client-side (HMAC-SHA1 over the canonical request).
//!
//! The `Storage` port lives in `kokkak_domain::storage`; the
//! application layer is oblivious to the concrete adapter.

pub mod memory;
pub mod s3;

pub use memory::MemoryStorage;
pub use s3::{S3Config, S3Storage};
