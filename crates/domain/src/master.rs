//! Master-data DTOs (M20+).
//!
//! Shared reference data consumed by every client (mobile,
//! customer web, admin web). Lives in `domain` per AGENTS.md §6.
//!
//! ## Why a generic shape (`value` / `label`) instead of a typed
//! struct per master table
//!
//! The same wire contract is reused across every dropdown: clients
//! pattern-match on `value` (the stable GUID string) and surface
//! `label` in the UI. Adding `MasterCountry { code, dial_code, ... }`
//! later for a richer admin column view doesn't break the dropdown
//! endpoint — a future `CountryDetail` DTO sits next to this one.
//!
//! ponytail: deliberately thin (`String` x2). No enum mapping, no
//! language fallback — the SP returns whatever name it stores
//! (Thai by default; an i18n layer lands in M20+ when a second
//! locale is in `master_country_*_en`). Ceiling: when a master
//! type needs a wider payload (e.g. provinces with `region_code`),
//! add a sibling struct rather than overloading this one.

use serde::{Deserialize, Serialize};

/// One row of a master-data dropdown (label / value pair).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MasterDropdownRow {
    /// Stable identifier — the master table's GUID string. Front-ends
    /// submit this when creating an order / profile so the binding
    /// survives a label rename.
    pub value: String,
    /// Human-readable label shown in the dropdown UI (single locale;
    /// see the module docs for the i18n ceiling).
    pub label: String,
}

/// One row of the master-position autocomplete.
///
/// Sibling to [`MasterDropdownRow`] (not a widening of it) per the
/// module-level ceiling doc: the autocomplete payload carries the
/// extra columns (`code`, `level`, `description`, `status`,
/// joined team + department) that the admin search box needs to
/// render rich results, but the simple dropdown never requires.
///
/// `value` / `label` are intentionally duplicated from the SP's
/// `master_position_guid` / `master_position_name` aliases so the
/// admin UI can drop the row straight into its generic dropdown
/// widget without re-shaping.
///
/// The joined `user_department_team_*` / `user_department_*` fields
/// are populated by the SP's `LEFT JOIN` and stay empty strings when
/// the position has no team (NULL FK + no matching row). They let the
/// admin UI render `position — team — department` breadcrumbs without
/// a second lookup, mirroring the `UserDepartmentTeamAutocompleteRow`
/// shape so a generic `<Autocomplete>` component on the admin web
/// can render both pickers the same way.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MasterPositionAutocompleteRow {
    /// Stable identifier — `master_position_guid` (GUID string).
    pub value: String,
    /// Human-readable label — `master_position_name`.
    pub label: String,
    /// Short admin code (`master_position_code`).
    pub code: String,
    /// Description shown in the autocomplete tooltip
    /// (`master_position_description`). Empty string when NULL.
    pub description: String,
    /// Hierarchy / rank level (`master_position_level`).
    /// `0` when NULL — positions without an explicit level sort
    /// last under the SP's `ORDER BY master_position_level DESC`.
    pub level: i32,
    /// Row status (`master_position_status`). `1` = active, `0` =
    /// inactive, `2` = any other non-deleted. The autocomplete SP
    /// already filters to `status = 1`; the field is forwarded so
    /// the UI can render an "(inactive)" badge if the row sneaks in
    /// after a future SP change.
    pub status: i32,
    /// `user_department_team.user_department_team_guid` — parent
    /// team's GUID. Empty string when the position has no team
    /// (LEFT JOIN miss).
    pub user_department_team_guid: String,
    /// `user_department_team.user_department_team_code` (admin-facing).
    /// Empty string when no team.
    pub user_department_team_code: String,
    /// `user_department_team.user_department_team_name` — breadcrumb
    /// line. Empty string when no team.
    pub user_department_team_name: String,
    /// `user_department.user_department_guid` — grandparent
    /// department GUID. Empty string when no team (or team has no
    /// department).
    pub user_department_guid: String,
    /// `user_department.user_department_code` (admin-facing).
    /// Empty string when no department.
    pub user_department_code: String,
    /// `user_department.user_department_name` — secondary line.
    /// Empty string when no department.
    pub user_department_name: String,
}

/// One row of the `user_department_team` autocomplete.
///
/// Richer than [`MasterDropdownRow`] because the autocomplete UI
/// surfaces the parent department alongside the team so the user
/// can disambiguate teams that share a name across departments.
/// `value` / `label` mirror the dropdown contract so a generic
/// `<Autocomplete>` component on the admin web can render the
/// pair, while the rest of the fields populate the secondary
/// line / hover tooltip.
///
/// ponytail: typed fields, no enum mapping. The status is kept as
/// `i32` (not a Rust enum) because the SP owns the value list and
/// the admin UI already understands the legacy numeric codes — a
/// Rust-side mapping would silently drift from the SP. Ceiling:
/// when a second autocomplete type needs its own shape, follow
/// this struct with another sibling — don't unify into a
/// generic `MasterAutocompleteRow` with optional fields (the
/// `Option` soup would force every caller to defensive-match).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UserDepartmentTeamAutocompleteRow {
    /// `user_department_team.user_department_team_guid` — stable id.
    pub value: String,
    /// `user_department_team.user_department_team_name` — primary label.
    pub label: String,
    /// Same as `value`, echoed for callers that prefer the long name.
    pub user_department_team_guid: String,
    /// `user_department_team.user_department_team_code` (admin-facing).
    pub user_department_team_code: String,
    /// Same as `label`, echoed for callers that prefer the long name.
    pub user_department_team_name: String,
    /// `user_department_team.user_department_team_status` (1 = active,
    /// 0 = inactive). The SP filters to active-only; kept on the row
    /// so the admin UI can grey-out an item without a second query.
    pub user_department_team_status: i32,
    /// `user_department.user_department_guid` — parent department id.
    pub user_department_guid: String,
    /// `user_department.user_department_code` (admin-facing).
    pub user_department_code: String,
    /// `user_department.user_department_name` — secondary line.
    pub user_department_name: String,
}
