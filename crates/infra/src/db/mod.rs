//! Database clients (ไคลเอนต์ฐานข้อมูล).
//!
//! - `mssql` (T06): SQL Server via `tiberius` + `bb8-tiberius` pool — stub.
//! - `mongo` (T09): MongoDB via `mongodb` driver.
//! - `migrate` (T09): versioned SQL migration runner.
//! - `json` (M1.5): generic JSON-file-backed store used to simulate
//!   the relational DB in dev while we wire M2 / M3 use cases.
//! - `json_user` / `json_catalog` / `json_order` (M2 / M3): concrete
//!   `UserRepository` / `ServiceRepository` / `OrderRepository`
//!   implementations backed by [`json::JsonStore`].
//! - `json_chat` (M8) / `mongo_chat` (M8): chat persistence.
//! - `json_payment` (M9): payment persistence.

pub mod json;
pub mod json_catalog;
pub mod json_chat;
pub mod json_order;
pub mod json_payment;
pub mod json_user;
pub mod migrate;
pub mod mongo;
pub mod mongo_chat;
pub mod mssql;
pub mod mssql_catalog;
pub mod mssql_order;
pub mod mssql_user;
