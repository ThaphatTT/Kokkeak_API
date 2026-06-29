//! Master-data repository port (M20+).
//!
//! Centralised read-side port for master-data dropdowns consumed by
//! every client (mobile, customer web, admin web). Adapters call the
//! `dbo.SP_MASTER_*_DROPDOWN_GET` family of stored procedures defined
//! in `migrations/2026062*_sp_master_*.sql`.
//!
//! ## Trait shape — one method per master type
//!
//! Each master table gets its own method. The wire DTO is the same
//! [`MasterDropdownRow`] for all of them — clients pattern-match on
//! `value` regardless of which master type is in play. The advantage
//! is type-safe Rust callers (no string-keyed dispatch); the cost is
//! a new method per type, which is exactly the same cost as adding
//! a new SP. **Adding a province dropdown later means: (a) write
//! `SP_MASTER_PROVINCE_DROPDOWN_GET`, (b) add `list_provinces(&str)`
//! to this trait, (c) implement in the infra adapter, (d) wire a
//! service + handler.**
//!
//! ## Filter rules (uniform across master types)
//!
//! - `keyword` is `None` or blank → no filter.
//! - `keyword` is `Some(text)` → SP applies LIKE on the relevant
//!   columns (name + code, by convention).
//! - `status` is `None` → SP applies its own default (e.g. `1` =
//!   active). `Some(0/1/2)` overrides; `Some(3)` (deleted) is
//!   hard-excluded by every SP in this family.
//!
//! ## No `caller_guid` admin gate (yet)
//!
//! Master data is shared reference data; the M19 admin gate
//! doesn't apply. If a future master-data SP becomes
//! admin-only (e.g. internal taxonomy editing), add a
//! `caller_guid: Uuid` parameter per the M19 contract.

use async_trait::async_trait;

use crate::master::MasterDropdownRow;
use crate::traits::user::RepoError;

/// Repository contract for master-data dropdowns.
#[async_trait]
pub trait MasterDropdownRepository: Send + Sync {
    /// Country dropdown (`master_country` table).
    ///
    /// Backed by `dbo.SP_MASTER_COUNTRY_DROPDOWN_GET`. See the SP
    /// header for filter semantics. Returns ALL matching rows;
    /// the handler is responsible for any cap / pagination we
    /// may need later (today the country list is bounded at a
    /// few hundred rows — no pagination).
    async fn list_countries(
        &self,
        keyword: Option<&str>,
        status: Option<i32>,
    ) -> Result<Vec<MasterDropdownRow>, RepoError>;
}
