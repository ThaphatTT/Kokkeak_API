

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

async fn lookup_user_guid_by_username(pool: &MssqlPool, username: &str) -> Option<Uuid> {
    use tiberius::ToSql;

    let params: &[&dyn ToSql] = &[&username as &dyn ToSql];
    let rows = kokkak_infra::db::mssql::exec_sp(
        pool,
        "SELECT user_guid FROM dbo.[user] WHERE user_username = @P1",
        params,
    )
    .await
    .ok()?;
    let row = rows.first()?;

    let guid_str: String = row.get::<&str, _>("user_guid")?.to_string();
    Uuid::parse_str(&guid_str).ok()
}
