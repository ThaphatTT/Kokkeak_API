//! `TranslationRepository` port (พอร์ตที่เก็บคำแปล — M11).
//!
//! Lets the API override the bundled `rust_i18n` catalog with
//! per-tenant translations loaded from a database. The default
//! adapter is the in-memory JSON map; production deploys swap in
//! a SQL Server / MongoDB-backed implementation when those land
//! in M12+ (see `kokkak_infra::db::mssql_translation`).
//!
//! ## Resolution order
//!
//! 1. Per-tenant DB override (`repo.get(locale, key)`)
//! 2. File-based default (`crates/common/locales/{en,th,lo}.yml`)
//!
//! The caller is responsible for falling through to the file
//! catalog when `get` returns `Ok(None)`. See
//! `kokkak_common::i18n::tr_with_repo` for the canonical
//! implementation.

use async_trait::async_trait;
use thiserror::Error;

/// Failure modes for the translation store.
#[derive(Debug, Error)]
pub enum TranslationError {
    /// The repository could not fulfil the lookup due to a
    /// driver / network / IO problem. The HTTP layer should
    /// fall through to the file catalog rather than 500.
    #[error("translation backend error: {0}")]
    Backend(String),
}

/// Translation lookup port.
///
/// Implementations must be **read-mostly**: hot lookups dominate
/// by orders of magnitude, writes are admin-only and rare. The
/// default M11 adapter (`JsonTranslationRepository`) keeps the
/// map in memory and persists to disk on `put`. A future MSSQL
/// adapter will rely on a covering index `(locale, key)` and a
/// small moka L1 cache.
#[async_trait]
pub trait TranslationRepository: Send + Sync {
    /// Look up a single translation. Returns:
    /// - `Ok(Some(value))` — override present, use this
    /// - `Ok(None)` — no override, fall through to the file catalog
    /// - `Err(_)` — the lookup itself failed (driver error, etc.)
    ///
    /// Callers should treat `Err` as "use the file fallback" so
    /// a transient DB blip doesn't surface as a 500 to the user.
    async fn get(&self, locale: &str, key: &str) -> Result<Option<String>, TranslationError>;

    /// Insert or update an override. The default in-memory
    /// adapter is idempotent; the MSSQL adapter will be too
    /// (UPSERT on `(locale, key)`).
    async fn put(&self, locale: &str, key: &str, value: &str) -> Result<(), TranslationError>;

    /// Number of overrides currently held. Used by `/admin/i18n`
    /// (planned for M12) to render a dashboard; not on the hot
    /// path.
    async fn count(&self) -> Result<usize, TranslationError> {
        Ok(0)
    }
}
