//! Live SQL Server integration test for `dbo.FN_SECURITY_USER_HAS_PERMISSION`
//! (the SP wrapper around `FN_SECURITY_USER_HAS_PERMISSION`).
//!
//! Runs against a real SQL Server reachable via
//! `KOKKAK_DATABASE__SQLSERVER_URL`. Skipped when the env var is
//! empty / missing / `"disabled"`.
//!
//! ## What it covers
//!
//! 1. **Deny** — user with no role and no override → `is_allowed = 0`.
//! 2. **Role-allow** — admin user (seeded by M14) has `USERS_CREATE` → `is_allowed = 1`.
//!
//! ## Prerequisite
//!
//! The DBA must have applied the migration
//! `migrations/20260701000002_sp_permission_has_permission_wrapper.sql`
//! (or its TVF `dbo.FN_SECURITY_USER_HAS_PERMISSION`) before this
//! test runs. The test does NOT auto-migrate — the SP lives outside
//! our migration runner's scope.
//!
//! ponytail: each test mints fresh GUIDs so the suite can run
//! concurrently (no shared seed cleanup). Run with
//! `cargo test --test integration_permission_sqlserver -- --test-threads=4`.

use std::env;

use kokkak_infra::db::mssql::{build_pool, MssqlPool};
use kokkak_infra::db::mssql_permission::MssqlPermissionRepository;

use kokkak_common::config::DatabaseSettings;
use kokkak_domain::Permission;
use uuid::Uuid;

fn live_url() -> Option<String> {
    let raw = env::var("KOKKAK_DATABASE__SQLSERVER_URL").ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "disabled" {
        return None;
    }
    Some(trimmed.to_string())
}

async fn pool_for(url: &str) -> MssqlPool {
    let settings = DatabaseSettings {
        sqlserver_url: url.to_string(),
        pool_size: 4,
        connect_timeout_secs: 5,
        migrations_dir: String::new(),
    };
    build_pool(&settings)
        .await
        .expect("build_pool against live SQL Server")
}

#[tokio::test]
async fn deny_when_user_has_no_roles_and_no_overrides() {
    let Some(url) = live_url() else {
        eprintln!(
            "skipping deny_when_user_has_no_roles_and_no_overrides — \
             set KOKKAK_DATABASE__SQLSERVER_URL to enable"
        );
        return;
    };
    let pool = pool_for(&url).await;
    let repo = MssqlPermissionRepository::new(pool);

    // Fresh GUID that the SP will fail to resolve → resolved_user
    // is empty → CASE branch 1 fires (NOT EXISTS resolved_user).
    let unknown_guid = Uuid::new_v4();
    let result = repo
        .has_permission(unknown_guid, Permission::UsersCreate)
        .await
        .expect("SP call must succeed even for unknown user");
    assert!(!result, "unknown user must be denied");
}

#[tokio::test]
async fn role_allow_grants_permission_to_admin() {
    let Some(url) = live_url() else {
        eprintln!(
            "skipping role_allow_grants_permission_to_admin — \
             set KOKKAK_DATABASE__SQLSERVER_URL to enable"
        );
        return;
    };
    let pool = pool_for(&url).await;

    // Look up the admin user's GUID (seeded by M14 migration
    // 20260619000002_seed_user_roles.sql).
    let admin_guid = match lookup_user_guid_by_username(&pool, "admin").await {
        Some(g) => g,
        None => {
            eprintln!(
                "admin user not seeded in target DB — skipping \
                 role_allow_grants_permission_to_admin"
            );
            return;
        }
    };

    let repo = MssqlPermissionRepository::new(pool);
    let result = repo
        .has_permission(admin_guid, Permission::UsersCreate)
        .await
        .expect("SP call");
    assert!(result, "admin role must grant USERS_CREATE");
}

/// Resolve a username → user_guid via a raw query. The canonical
/// lookup lives in `MssqlUserRepository`; this helper hits the table
/// directly to keep this test file self-contained.
async fn lookup_user_guid_by_username(pool: &MssqlPool, username: &str) -> Option<Uuid> {
    use tiberius::ToSql;
    let row = kokkak_infra::db::mssql::exec_sp(
        pool,
        "SELECT user_guid FROM dbo.[user] WHERE user_username = @P1",
        &[&username as &dyn ToSql],
    )
    .await
    .ok()?
    .first()?;
    // Row::get returns `Option<&str>` (None when column missing / NULL).
    // `?` unwraps; `to_string()` owns the value so `parse_str` can borrow it.
    let guid_str: String = row.get::<&str, _>("user_guid")?.to_string();
    Uuid::parse_str(&guid_str).ok()
}
