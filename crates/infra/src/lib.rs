//! Infra layer
//!
//! Concrete implementations of the repository traits defined in `domain`:
//! SQL Server via `tiberius`, MongoDB, Redis, NATS, FCM, S3, etc.
//!
//! Also houses the SQL migration runner (since `sqlx-cli` does not
//! support MSSQL).

#![deny(unsafe_code)]
#![warn(missing_docs)]
