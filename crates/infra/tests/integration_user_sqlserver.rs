//! Live SQL Server integration test for the 4-table NEW_DB user schema (M14).
//!
//! Runs against a real SQL Server reachable via
//! `KOKKAK_DATABASE__SQLSERVER_URL`. Skipped when the env var is
//! empty / missing / `"disabled"`.
//!
//! ## What it covers
//!
//! 1. **Migration** — runs the discovered `.sql` files in
//!    `migrations/` (idempotent; safe to run repeatedly).
//! 2. **Insert** — writes to `[user]` + `[user_username]` +
//!    `[user_user_role]` in one transaction.
//! 3. **Read** — single JOIN query returns the `User` aggregate
//!    with `roles: Vec<Role>` correctly assembled.
//! 4. **Conflict** — duplicate username returns `RepoError::Conflict`.
//! 5. **Update** — password + status changes persist through the
//!    update path.
//!
//! ponytail: shared test pool + unique GUIDs / usernames per test
//! avoid cleanup. Run with `cargo test --test integration_user_sqlserver -- --test-threads=1`
//! against a real SQL Server to gate M14 release.

use std::env;
use std::time::Duration;

use kokkak_infra::db::migrate;
use kokkak_infra::db::mssql::{build_pool, MssqlPool};
use kokkak_infra::db::mssql_user::MssqlUserRepository;

use kokkak_common::config::DatabaseSettings;
use kokkak_domain::{RepoError, Role, User, UserRepository, UserStatus};

use chrono::Utc;
use uuid::Uuid;

/// Read `KOKKAK_DATABASE__SQLSERVER_URL`; return `None` when the
/// live integration test should be skipped.
fn live_url() -> Option<String> {
    let raw = env::var("KOKKAK_DATABASE__SQLSERVER_URL").ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "disabled" {
        return None;
    }
    Some(trimmed.to_string())
}

/// Build a one-off pool from the live URL. Each test gets its own
/// pool so they don't fight for the connection budget.
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

/// Run the versioned migrations against `pool` once. Idempotent:
/// the runner tracks applied versions in `schema_migrations` so
/// subsequent runs are no-ops.
async fn ensure_schema(pool: &MssqlPool) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations");
    let applied = migrate::run(pool, &dir)
        .await
        .expect("run migrations against live SQL Server");
    eprintln!("live_user_sqlserver: applied {} new migration(s)", applied);
}

/// Build a sample user with a unique username (so repeated runs
/// don't collide on the `[user_username]` UNIQUE constraint).
fn sample_user(role: Role) -> User {
    let now = Utc::now();
    User {
        id: Uuid::new_v4(),
        first_name: "Live".into(),
        last_name: "Tester".into(),
        username: format!("live-{}@example.com", Uuid::new_v4()),
        password_hash: "$argon2id$v=19$m=65536,t=3,p=4$test$test".into(),
        roles: vec![role],
        permissions: Vec::new(),
        status: UserStatus::Active,
        created_at: now,
        updated_at: now,
    }
}

/// Helper: insert, then immediately find_by_username and verify
/// the round-trip. Returns the freshly-inserted user.
async fn insert_and_verify(repo: &MssqlUserRepository, user: User) -> User {
    repo.insert(&user).await.expect("insert");
    let by_username = repo
        .find_by_username(&user.username)
        .await
        .expect("find_by_username")
        .expect("user must exist after insert");
    assert_eq!(by_username.id, user.id);
    assert_eq!(by_username.username, user.username);
    assert_eq!(by_username.first_name, user.first_name);
    assert_eq!(by_username.last_name, user.last_name);
    assert_eq!(by_username.password_hash, user.password_hash);
    assert_eq!(by_username.status, user.status);
    assert_eq!(
        by_username.created_at.timestamp(),
        user.created_at.timestamp()
    );
    assert_eq!(by_username.roles, user.roles);

    // find_by_id must hit the same row.
    let by_id = repo
        .find_by_id(user.id)
        .await
        .expect("find_by_id")
        .expect("user must exist by id");
    assert_eq!(by_id.username, user.username);
    assert_eq!(by_id.roles, user.roles);

    by_username
}

#[tokio::test]
async fn live_register_then_find_round_trip() {
    let Some(url) = live_url() else {
        eprintln!(
            "skipping live_register_then_find_round_trip: \
             KOKKAK_DATABASE__SQLSERVER_URL not set"
        );
        return;
    };
    let pool = pool_for(&url).await;
    ensure_schema(&pool).await;
    let repo = MssqlUserRepository::new(pool);
    let user = sample_user(Role::Customer);
    let _found = insert_and_verify(&repo, user).await;
}

#[tokio::test]
async fn live_technician_role_round_trip() {
    let Some(url) = live_url() else {
        return;
    };
    let pool = pool_for(&url).await;
    ensure_schema(&pool).await;
    let repo = MssqlUserRepository::new(pool);
    let user = sample_user(Role::Technician);
    let found = insert_and_verify(&repo, user).await;
    assert_eq!(found.roles, vec![Role::Technician]);
}

#[tokio::test]
async fn live_admin_role_round_trip() {
    let Some(url) = live_url() else {
        return;
    };
    let pool = pool_for(&url).await;
    ensure_schema(&pool).await;
    let repo = MssqlUserRepository::new(pool);
    let user = sample_user(Role::Admin);
    let found = insert_and_verify(&repo, user).await;
    assert_eq!(found.roles, vec![Role::Admin]);
    assert!(found.is_admin());
    assert!(!found.is_super_admin());
}

#[tokio::test]
async fn live_super_admin_role_round_trip() {
    let Some(url) = live_url() else {
        return;
    };
    let pool = pool_for(&url).await;
    ensure_schema(&pool).await;
    let repo = MssqlUserRepository::new(pool);
    let user = sample_user(Role::SuperAdmin);
    let found = insert_and_verify(&repo, user).await;
    assert_eq!(found.roles, vec![Role::SuperAdmin]);
    assert!(found.is_super_admin());
}

#[tokio::test]
async fn live_find_by_username_is_case_insensitive() {
    let Some(url) = live_url() else {
        return;
    };
    let pool = pool_for(&url).await;
    ensure_schema(&pool).await;
    let repo = MssqlUserRepository::new(pool);
    let mut user = sample_user(Role::Customer);
    user.username = format!("Mixed-{}@example.com", Uuid::new_v4());
    repo.insert(&user).await.expect("insert");
    let found = repo
        .find_by_username(&user.username.to_uppercase())
        .await
        .expect("find_by_username")
        .expect("case-insensitive lookup must succeed");
    assert_eq!(found.id, user.id);
}

#[tokio::test]
async fn live_duplicate_username_returns_conflict() {
    let Some(url) = live_url() else {
        return;
    };
    let pool = pool_for(&url).await;
    ensure_schema(&pool).await;
    let repo = MssqlUserRepository::new(pool);
    let user = sample_user(Role::Customer);
    let username = user.username.clone();
    repo.insert(&user).await.expect("first insert");

    let mut dup = sample_user(Role::Customer);
    dup.username = username.clone();
    let err = repo
        .insert(&dup)
        .await
        .expect_err("duplicate must conflict");
    match err {
        RepoError::Conflict(msg) => {
            assert!(
                msg.contains(&username),
                "message should name the username: {msg}"
            );
        }
        other => panic!("expected Conflict, got {other:?}"),
    }
}

#[tokio::test]
async fn live_update_persists_new_password_and_status() {
    let Some(url) = live_url() else {
        return;
    };
    let pool = pool_for(&url).await;
    ensure_schema(&pool).await;
    let repo = MssqlUserRepository::new(pool);
    let mut user = sample_user(Role::Customer);
    repo.insert(&user).await.expect("insert");

    user.password_hash = "$argon2id$ROTATED".into();
    user.first_name = "Rotated".into();
    user.last_name = "Name".into();
    user.status = UserStatus::Suspended;
    user.updated_at = Utc::now() + Duration::from_secs(1);
    repo.update(&user).await.expect("update");

    let found = repo
        .find_by_username(&user.username)
        .await
        .expect("find_by_username")
        .expect("user must still exist");
    assert_eq!(found.password_hash, "$argon2id$ROTATED");
    assert_eq!(found.first_name, "Rotated");
    assert_eq!(found.last_name, "Name");
    assert_eq!(found.status, UserStatus::Suspended);
    assert!(!found.can_authenticate(), "suspended user cannot login");
}

#[tokio::test]
async fn live_update_missing_returns_not_found() {
    let Some(url) = live_url() else {
        return;
    };
    let pool = pool_for(&url).await;
    ensure_schema(&pool).await;
    let repo = MssqlUserRepository::new(pool);
    let user = sample_user(Role::Customer);
    let err = repo.update(&user).await.expect_err("missing user");
    assert!(matches!(err, RepoError::NotFound(_)));
}

#[tokio::test]
async fn live_find_unknown_username_returns_none() {
    let Some(url) = live_url() else {
        return;
    };
    let pool = pool_for(&url).await;
    ensure_schema(&pool).await;
    let repo = MssqlUserRepository::new(pool);
    let got = repo
        .find_by_username(&format!("ghost-{}@example.com", Uuid::new_v4()))
        .await
        .expect("find_by_username");
    assert!(got.is_none());
}
