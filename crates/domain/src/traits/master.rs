//! Master-data repository port (M20+).
//!
//! Centralised read-side port for master-data lookups consumed by
//! every client (mobile, customer web, admin web). Adapters call one
//! of two SP families defined in `migrations/2026062*_sp_master_*.sql`
//! and `migrations/2026070*_sp_autocomplete_*.sql`:
//!
//! - `dbo.SP_MASTER_*_DROPDOWN_GET` — **dropdown** family: full
//!   bounded list, filter by `@p_*_status` (e.g. `country`).
//! - `dbo.SP_AUTOCOMPLETE_*_GET` — **typeahead** family: bounded by
//!   `@p_take`, prefix-match on `name` + `code` (e.g. `master_position`,
//!   `user_department`, `user_department_team`).
//!
//! The two families are separate because the filter semantics differ
//! (status gate vs take cap). Autocomplete results that return
//! richer payloads (e.g. `master_position` carries `code` / `level`
//! / `description` / `status`) expose a sibling DTO; vanilla
//! autocompletes that only need `value` / `label` reuse the
//! dropdown [`MasterDropdownRow`] contract so clients pattern-match
//! on `value` regardless of which master type is in play.
//!
//! ## Trait shape — one method per master type
//!
//! Each master table gets its own method. The advantage is type-safe
//! Rust callers (no string-keyed dispatch); the cost is a new method
//! per type, which is exactly the same cost as adding a new SP.
//! **Adding a province dropdown later means: (a) write
//! `SP_MASTER_PROVINCE_DROPDOWN_GET`, (b) add `list_provinces(&str)`
//! to this trait, (c) implement in the infra adapter, (d) wire a
//! service + handler.** Same recipe for autocomplete additions
//! using `SP_AUTOCOMPLETE_<TYPE>_GET`.
//!
//! ## Filter rules
//!
//! **Dropdown family** (`status` filter):
//! - `keyword` is `None` or blank → no filter.
//! - `keyword` is `Some(text)` → SP applies LIKE on the relevant
//!   columns (name + code, by convention).
//! - `status` is `None` → SP applies its own default (e.g. `1` =
//!   active). `Some(0/1/2)` overrides; `Some(3)` (deleted) is
//!   hard-excluded by every SP in this family.
//!
//! **Autocomplete family** (`take` filter):
//! - `keyword` is `None` or blank → top `@p_take` rows (no filter).
//! - `keyword` is `Some(text)` → SP applies prefix-LIKE on name +
//!   code (typeahead UX, not substring).
//! - `take` is `None` → SP applies default (20). `Some(n)` clamps
//!   to `[1, 100]` inside the SP — the Rust layer does **not**
//!   duplicate this logic (see infra adapter for the rationale).
//!
//! ## No `caller_guid` admin gate (yet)
//!
//! Master data is shared reference data; the M19 admin gate
//! doesn't apply. If a future master-data SP becomes
//! admin-only (e.g. internal taxonomy editing), add a
//! `caller_guid: Uuid` parameter per the M19 contract.

use async_trait::async_trait;

use crate::master::{
    MasterDropdownRow, MasterPositionAutocompleteRow, UserDepartmentTeamAutocompleteRow,
};
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

    /// User-department autocomplete (`user_department` table).
    ///
    /// Backed by `dbo.SP_AUTOCOMPLETE_USER_DEPARTMENT_GET`. See the
    /// SP header for filter semantics:
    ///
    /// - `keyword` is `None` or blank → top `take` rows (no filter).
    /// - `keyword` is `Some(text)` → prefix-match on
    ///   `user_department_name` + `user_department_code`.
    /// - `take` is `None` → SP default (20). `Some(n)` with `n <= 0`
    ///   → SP default (20); `Some(n > 100)` → SP clamps to 100.
    ///   The infra adapter re-clamps `take` to `[1, 100]` so the
    ///   trait contract is self-documenting (mirrors the
    ///   `autocomplete_user_department_team` pattern).
    ///
    /// The wire DTO is the same [`MasterDropdownRow`] — clients
    /// pattern-match on `value` regardless of which master type is
    /// in play (country vs user_department share the contract).
    async fn autocomplete_user_department(
        &self,
        keyword: Option<&str>,
        take: Option<i32>,
    ) -> Result<Vec<MasterDropdownRow>, RepoError>;

    /// Master-position autocomplete (`master_position` table).
    ///
    /// Backed by `dbo.SP_AUTOCOMPLETE_MASTER_POSITION_GET`. Returns
    /// at most `take` rows (SP clamps `1..=100`, default `20`,
    /// `None` → `20`). The result carries the extra columns
    /// (`code`, `description`, `level`, `status`) so the admin UI
    /// can render rich autocomplete results, not just a label/value
    /// pair — hence the sibling DTO [`MasterPositionAutocompleteRow`]
    /// instead of the generic [`MasterDropdownRow`].
    ///
    /// `keyword: None` or blank → top `take` active rows (SP
    /// no-filter branch). `keyword: Some(text)` → prefix-LIKE on
    /// `master_position_name` + `master_position_code`. The SP
    /// already filters to `status = 1`; no status param.
    async fn autocomplete_master_positions(
        &self,
        keyword: Option<&str>,
        take: Option<i32>,
    ) -> Result<Vec<MasterPositionAutocompleteRow>, RepoError>;

    /// Autocomplete lookup for the admin user-form's
    /// `user_department_team` picker.
    ///
    /// Backed by `dbo.SP_AUTOCOMPLETE_USER_DEPARTMENT_TEAM_GET`.
    /// The SP applies its own defaults (`take = 20`, capped at
    /// `100`, hard-coded `status = 1` active-only), so a `None`
    /// on every filter returns the first 20 active rows for every
    /// department — the typical "show me the recent teams" view
    /// on the admin form.
    ///
    /// The result carries the parent department alongside the team
    /// so the UI can disambiguate teams that share a name across
    /// departments — hence the sibling DTO
    /// [`UserDepartmentTeamAutocompleteRow`].
    ///
    /// - `user_department_guid`: `None` / blank → no department
    ///   filter (every team across every department); `Some(guid)`
    ///   narrows to a single department.
    /// - `keyword`: `None` / blank → no text filter; `Some(text)`
    ///   LIKE-matches against team name / code and department
    ///   name / code.
    /// - `take`: `None` → SP default (20); `Some(n)` with `n ≤ 0`
    ///   → SP default (20); `Some(n > 100)` → SP clamps to 100.
    async fn autocomplete_user_department_team(
        &self,
        user_department_guid: Option<&str>,
        keyword: Option<&str>,
        take: Option<i32>,
    ) -> Result<Vec<UserDepartmentTeamAutocompleteRow>, RepoError>;
}
