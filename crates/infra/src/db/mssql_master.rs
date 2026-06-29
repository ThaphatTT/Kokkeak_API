//! SQL Server-backed `MasterDropdownRepository` (M20).
//!
//! Implements [`kokkak_domain::MasterDropdownRepository`] via tiberius +
//! the NEW_DB v2 `SP_MASTER_*_DROPDOWN_GET` family (country first; the
//! same pattern slots in provinces / banks / etc. as they land).
//! **No inline SQL** — every operation is `EXEC dbo.SP_MASTER_*`.
//!
//! ## Trait ↔ SP shape
//!
//! Each trait method is one SP call. The wire DTO is the same
//! `MasterDropdownRow` (`value`, `label`) for every dropdown, so the
//! mapper is intentionally thin (one helper, shared by every method).
//!
//! ## Filter encoding
//!
//! - `keyword: None` → bind `Option<&str> = None` so tiberius sends
//!   SQL `NULL`. The SP's `OR @p_keyword IS NULL` branch makes this
//!   the no-filter case.
//! - `status: None` → bind `Option<&i32> = Some(1)` so the SP's
//!   default-equivalent (active-only) kicks in even when tiberius
//!   serialises the param.
//! - `status: Some(0)` → bind `0_i32` (inactive rows included; status=3
//!   is hard-excluded by the SP).

use async_trait::async_trait;
use tiberius::ToSql;

use kokkak_domain::traits::master::MasterDropdownRepository;
use kokkak_domain::traits::user::RepoError;
use kokkak_domain::MasterDropdownRow;

use crate::db::mssql::{exec_sp, read_str, MssqlPool};

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
