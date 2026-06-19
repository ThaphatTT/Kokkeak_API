//! Database clients (ไคลเอนต์ฐานข้อมูล — M14.5+).
//!
//! M14.5 dropped the JSON-DB simulation in favour of real SQL Server
//! stored procedures. The only persistence adapters are:
//!
//! - `mssql` — pool + stored-procedure executor (`exec_sp`, `read_*`).
//! - `mssql_user` / `mssql_catalog` / `mssql_order` / `mssql_chat` /
//!   `mssql_payment` / `mssql_translation` — per-aggregate repos that
//!   call `EXEC dbo.API_*` against the NEW_DB v2 schema.
//! - `topology` (M12): multi-DB pool registry keyed by [`DbRole`].
//! - `mongo` (T09): MongoDB via `mongodb` driver (M8 chat fallback).
//! - `migrate` (T09): versioned SQL migration runner.

pub mod migrate;
pub mod mongo;
pub mod mongo_chat;
pub mod mssql;
pub mod mssql_catalog;
pub mod mssql_chat;
pub mod mssql_order;
pub mod mssql_payment;
pub mod mssql_translation;
pub mod mssql_user;
pub mod topology;
