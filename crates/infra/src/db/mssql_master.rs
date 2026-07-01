//! SQL Server-backed `MasterDropdownRepository` (M20).
//!
//! Implements [`kokkak_domain::MasterDropdownRepository`] via tiberius
//! + the NEW_DB v2 SP families:
//!
//! - `dbo.SP_MASTER_*_DROPDOWN_GET` — **dropdown** family (filter by
//!   status, returns all matching rows for the bounded list).
//! - `dbo.SP_AUTOCOMPLETE_*_GET`    — **typeahead** family (filtered
//!   by `@p_take`, prefix-match on `name` + `code`).
//!
//! **No inline SQL** — every operation is `EXEC dbo.SP_*`.
//!
//! ## Trait ↔ SP shape
//!
//! Each trait method is one SP call. Wire DTO varies: vanillas
//! autocompletes reuse [`MasterDropdownRow`] (`value`, `label`) so the
//! admin UI can reuse the dropdown contract; richer autocompletes
//! (`master_position`, `user_department_team`) get their own sibling
//! DTO via a per-method mapper.
//!
//! ## Filter encoding
//!
//! - `keyword: None` → bind `Option<&str> = None` so tiberius sends
//!   SQL `NULL`. The dropdown SP's `OR @p_keyword IS NULL` branch
//!   makes this the no-filter case. The autocomplete SP also has
//!   `IF @p_keyword = N''` branch → same effect.
//! - `status: None` on `list_countries` → bind `Option<i32> = Some(1)`
//!   so the SP's default-equivalent (active-only) kicks in even when
//!   tiberius serialises the param. Autocompletes do NOT have a
//!   `status` param — they hard-code `status = 1` in the SP.
//! - `take: None` on autocomplete → bind `None` and let the SP's
//!   `IF @p_take IS NULL OR @p_take <= 0` branch set 20.
//! - `take: Some(n)` on autocomplete — we mirror the SP's `[1, 100]`
//!   clamp here so the trait contract is self-documenting and a
//!   future SP regression still leaves us inside the documented bound.

use async_trait::async_trait;
use tiberius::ToSql;

use kokkak_domain::traits::master::MasterDropdownRepository;
use kokkak_domain::traits::user::RepoError;
use kokkak_domain::{
    MasterDropdownRow, MasterPositionAutocompleteRow, UserDepartmentTeamAutocompleteRow,
};

use crate::db::mssql::{exec_sp, read_i32, read_str, MssqlPool};

/// SQL Server-backed master-data dropdown repository (M20).
#[derive(Clone)]
pub struct MssqlMasterDropdownRepository {
    pool: MssqlPool,
}

impl MssqlMasterDropdownRepository {
    /// Construct the repository with a shared `MssqlPool`.
    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl MasterDropdownRepository for MssqlMasterDropdownRepository {
    async fn list_countries(
        &self,
        keyword: Option<&str>,
        status: Option<i32>,
    ) -> Result<Vec<MasterDropdownRow>, RepoError> {
        // status default: SP has `@p_master_country_status int = 1`;
        // tiberius's `Option<i32>` encodes `None` as SQL NULL which the
        // SP would interpret as "no filter" via the
        // `OR @p_master_country_status IS NULL` branch. We want
        // active-only (the typical caller) when the handler doesn't
        // specify anything — so the trait's `None` maps to `Some(1)`
        // here, NOT to SQL NULL. Pass `Some(0)` / `Some(2)` from the
        // handler to override; pass `_` (literal underscore) only when
        // the handler really wants "all non-deleted statuses", which
        // is rare for dropdowns.
        let status_param: Option<i32> = Some(status.unwrap_or(1));
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_MASTER_COUNTRY_DROPDOWN_GET \
                @p_keyword = @P1, @p_master_country_status = @P2",
            &[&keyword as &dyn ToSql, &status_param as &dyn ToSql],
        )
        .await?;
        Ok(rows.iter().map(row_to_master_dropdown_row).collect())
    }

    async fn autocomplete_user_department(
        &self,
        keyword: Option<&str>,
        take: Option<i32>,
    ) -> Result<Vec<MasterDropdownRow>, RepoError> {
        // take defaults: the SP sets `NULL` or non-positive → 20 and
        // clamps `> 100` → 100. We mirror the SP's `[1, 100]` clamp
        // here so the trait contract is self-documenting and a future
        // SP regression still leaves us inside the documented bound
        // (matches `autocomplete_user_department_team` pattern).
        let take_param: Option<i32> = Some(take.unwrap_or(20).clamp(1, 100));
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_AUTOCOMPLETE_USER_DEPARTMENT_GET \
                    @p_keyword = @P1, @p_take = @P2",
            &[&keyword as &dyn ToSql, &take_param as &dyn ToSql],
        )
        .await?;
        Ok(rows.iter().map(row_to_master_dropdown_row).collect())
    }

    async fn autocomplete_master_positions(
        &self,
        user_department_team_guid: Option<&str>,
        keyword: Option<&str>,
        take: Option<i32>,
    ) -> Result<Vec<MasterPositionAutocompleteRow>, RepoError> {
        // `take` mirrors the SP's `[1, 100]` clamp: the SP already does
        // it, but we double-enforce here so the Rust trait contract is
        // self-documenting and a future SP regression still leaves us
        // inside the documented bound. `None` → SP default (20).
        let take_param: Option<i32> = Some(take.unwrap_or(20).clamp(1, 100));
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_AUTOCOMPLETE_MASTER_POSITION_GET \
                    @p_department_team_guid = @P1, @p_keyword = @P2, @p_take = @P3",
            &[
                &user_department_team_guid as &dyn ToSql,
                &keyword as &dyn ToSql,
                &take_param as &dyn ToSql,
            ],
        )
        .await?;
        Ok(rows
            .iter()
            .map(row_to_master_position_autocomplete)
            .collect())
    }

    async fn autocomplete_user_department_team(
        &self,
        user_department_guid: Option<&str>,
        keyword: Option<&str>,
        take: Option<i32>,
    ) -> Result<Vec<UserDepartmentTeamAutocompleteRow>, RepoError> {
        // `take` defaults: the SP sets `NULL` or non-positive → 20 and
        // clamps `> 100` → 100. We mirror the SP's `[1, 100]` clamp
        // here so the trait contract is self-documenting and a future
        // SP regression still leaves us inside the documented bound.
        let take_param: Option<i32> = Some(take.unwrap_or(20).clamp(1, 100));
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_AUTOCOMPLETE_USER_DEPARTMENT_TEAM_GET \
                @p_user_department_guid = @P1, @p_keyword = @P2, @p_take = @P3",
            &[
                &user_department_guid as &dyn ToSql,
                &keyword as &dyn ToSql,
                &take_param as &dyn ToSql,
            ],
        )
        .await?;
        Ok(rows
            .iter()
            .map(row_to_user_department_team_autocomplete)
            .collect())
    }
}

/// Map a single `SP_MASTER_*_DROPDOWN_GET` row to
/// [`MasterDropdownRow`].
///
/// Column NAMES match the SP's SELECT aliases (one row per master item):
///   `value`   (varchar — GUID-string, M19 convention)
///   `label`   (nvarchar — display name)
///
/// `read_str` returns `Option<&str>` (NULL → None) which we coerce
/// to `String::new()` so the JSON wire shape stays stable when a
/// row arrives with NULL columns (defensive against a future SP
/// refactor).
fn row_to_master_dropdown_row(row: &tiberius::Row) -> MasterDropdownRow {
    MasterDropdownRow {
        value: read_str(row, "value").unwrap_or("").to_string(),
        label: read_str(row, "label").unwrap_or("").to_string(),
    }
}

/// Map a single `SP_AUTOCOMPLETE_MASTER_POSITION_GET` row to
/// [`MasterPositionAutocompleteRow`].
///
/// Column NAMES match the SP's SELECT aliases (the SP aliases the
/// position GUID / name as `value` / `label` so the admin UI can
/// reuse the dropdown contract on top):
///   `value`                       varchar GUID-string (position)
///   `label`                       nvarchar display name (position)
///   `master_position_guid`        varchar GUID-string
///   `master_position_code`        nvarchar
///   `master_position_name`        nvarchar
///   `master_position_description` nvarchar
///   `master_position_level`       int
///   `master_position_status`      int
///   `user_department_team_guid`   varchar GUID-string (joined team)
///   `user_department_team_code`   nvarchar
///   `user_department_team_name`   nvarchar
///   `user_department_guid`        varchar GUID-string (joined dept)
///   `user_department_code`        nvarchar
///   `user_department_name`        nvarchar
///
/// All string columns default to `""` on NULL (the SP's `LEFT JOIN`
/// can leave the team / department slots empty when a position has
/// no team, or the team has no parent department); `level` / `status`
/// default to `0` on NULL. This keeps the JSON wire shape stable
/// even when the SP returns partially-filled rows.
fn row_to_master_position_autocomplete(row: &tiberius::Row) -> MasterPositionAutocompleteRow {
    MasterPositionAutocompleteRow {
        value: read_str(row, "value").unwrap_or("").to_string(),
        label: read_str(row, "label").unwrap_or("").to_string(),
        code: read_str(row, "master_position_code")
            .unwrap_or("")
            .to_string(),
        description: read_str(row, "master_position_description")
            .unwrap_or("")
            .to_string(),
        level: read_i32(row, "master_position_level").unwrap_or(0),
        status: read_i32(row, "master_position_status").unwrap_or(0),
        user_department_team_guid: read_str(row, "user_department_team_guid")
            .unwrap_or("")
            .to_string(),
        user_department_team_code: read_str(row, "user_department_team_code")
            .unwrap_or("")
            .to_string(),
        user_department_team_name: read_str(row, "user_department_team_name")
            .unwrap_or("")
            .to_string(),
        user_department_guid: read_str(row, "user_department_guid")
            .unwrap_or("")
            .to_string(),
        user_department_code: read_str(row, "user_department_code")
            .unwrap_or("")
            .to_string(),
        user_department_name: read_str(row, "user_department_name")
            .unwrap_or("")
            .to_string(),
    }
}

/// Map a single `SP_AUTOCOMPLETE_USER_DEPARTMENT_TEAM_GET` row to
/// [`UserDepartmentTeamAutocompleteRow`].
///
/// Column NAMES match the SP's SELECT aliases (the SP aliases the
/// team GUID / name as `value` / `label` so the admin UI can
/// reuse the dropdown contract on top):
///   `value`                          varchar GUID-string (team)
///   `label`                          nvarchar display name (team)
///   `user_department_team_guid`      varchar GUID-string (team)
///   `user_department_team_code`      nvarchar
///   `user_department_team_name`      nvarchar
///   `user_department_team_status`    int
///   `user_department_guid`           varchar GUID-string (parent dept)
///   `user_department_code`           nvarchar
///   `user_department_name`           nvarchar
fn row_to_user_department_team_autocomplete(
    row: &tiberius::Row,
) -> UserDepartmentTeamAutocompleteRow {
    UserDepartmentTeamAutocompleteRow {
        value: read_str(row, "value").unwrap_or("").to_string(),
        label: read_str(row, "label").unwrap_or("").to_string(),
        user_department_team_guid: read_str(row, "user_department_team_guid")
            .unwrap_or("")
            .to_string(),
        user_department_team_code: read_str(row, "user_department_team_code")
            .unwrap_or("")
            .to_string(),
        user_department_team_name: read_str(row, "user_department_team_name")
            .unwrap_or("")
            .to_string(),
        user_department_team_status: read_i32(row, "user_department_team_status").unwrap_or(0),
        user_department_guid: read_str(row, "user_department_guid")
            .unwrap_or("")
            .to_string(),
        user_department_code: read_str(row, "user_department_code")
            .unwrap_or("")
            .to_string(),
        user_department_name: read_str(row, "user_department_name")
            .unwrap_or("")
            .to_string(),
    }
}
