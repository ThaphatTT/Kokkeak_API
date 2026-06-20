//! SQL Server-backed `TranslationRepository` (M14.5 — stored procedures).
//!
//! Replaces the M11 JSON file `translations.json` with a real DB table
//! (see `migrations/20260620000006_sp_translation.sql`).

use async_trait::async_trait;
use tiberius::ToSql;

use kokkak_domain::traits::translation::{TranslationError, TranslationRepository};

use crate::db::mssql::{exec_sp, MssqlPool};

/// SQL Server-backed `TranslationRepository` (M14.5 — stored procedures).
#[derive(Clone)]
pub struct MssqlTranslationRepository {
    pool: MssqlPool,
}

impl MssqlTranslationRepository {
    /// Construct the repository with a shared `MssqlPool`.
    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TranslationRepository for MssqlTranslationRepository {
    async fn get(&self, locale: &str, key: &str) -> Result<Option<String>, TranslationError> {
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.API_TRANSLATION_GET @p_locale = @P1, @p_key = @P2",
            &[&locale as &dyn ToSql, &key as &dyn ToSql],
        )
        .await
        .map_err(|e| TranslationError::Backend(e.to_string()))?;
        // First column of the first row = value.
        Ok(rows
            .first()
            .and_then(|r| r.get::<&str, _>(0))
            .map(|s| s.to_string()))
    }

    async fn put(&self, locale: &str, key: &str, value: &str) -> Result<(), TranslationError> {
        let system_user = uuid::Uuid::nil(); // system actor — M15+ uses real actor
        let _ = exec_sp(
            &self.pool,
            "EXEC dbo.API_TRANSLATION_PUT \
                @p_locale = @P1, @p_key = @P2, @p_value = @P3, @p_user_guid = @P4",
            &[
                &locale as &dyn ToSql,
                &key as &dyn ToSql,
                &value as &dyn ToSql,
                &system_user as &dyn ToSql,
            ],
        )
        .await
        .map_err(|e| TranslationError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn count(&self) -> Result<usize, TranslationError> {
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.API_TRANSLATION_LIST_BY_LOCALE @p_locale = @P1",
            &[&"th" as &dyn ToSql],
        )
        .await
        .map_err(|e| TranslationError::Backend(e.to_string()))?;
        Ok(rows.len())
    }
}
