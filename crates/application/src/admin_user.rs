//! Admin user use cases (M20-b).
//!
//! Wraps the rich admin-side user-creation stored procedure
//! (`dbo.SP_USER_INSERT_FULL`). Lives separately from
//! `AuthService::register` because:
//!
//! - The use case is **admin-initiated** (the actor is the JWT
//!   holder, an admin provisioning an account), not
//!   self-service registration.
//! - The SP expects the actor's `user_username_guid` (not
//!   `user_guid`), so the service performs a tiny extra lookup.
//! - The password must be hashed in Rust before the SP call —
//!   the SP receives an already-hashed argon2id PHC string.
//! - The SP emits ~24 distinct structured error codes that need
//!   to surface to the admin UI (see
//!   `crates/api/src/handlers/admin.rs::sp_insert_full_status`).
//!
//! ## Flow
//!
//! 1. **Resolve actor**: `find_username_guid_by_user_guid(jwt.id)`
//!    maps the JWT's `user_guid` → the SP's required
//!    `user_username_guid`. If the admin is missing / suspended,
//!    return [`AdminInsertUserError`] with code `ACTOR_NOT_FOUND`.
//! 2. **Validate working schedule**: if any day has
//!    `is_working = 1` but `start_time`/`end_time` is missing,
//!    short-circuit with `WORK_TIME_REQUIRED` (matches the SP's
//!    own check, surfaced as a 422 with the localized message
//!    before we burn a DB round-trip).
//! 3. **Hash the password** with [`PasswordHasherPort`]. The
//!    plaintext never reaches the DB driver.
//! 4. **Build** [`AdminInsertUserRequest`] and delegate to
//!    [`UserRepository::admin_insert_full`].
//!
//! ## M22-b: `admin_update_full`
//!
//! Write-side counterpart to M20-b. Same actor resolution +
//! schedule validation, but **no password hashing** — the
//! update SP never touches the password column. The handler
//! gates on [`Permission::UsersUpdate`]; the SP re-checks
//! `USERS_UPDATE` server-side as defense-in-depth.

use std::sync::Arc;

use chrono::{DateTime, Utc};
pub use kokkak_domain::admin_user::{
    AdminInsertUserError, AdminInsertUserRequest, AdminInsertUserResult, AdminUpdateUserError,
    AdminUpdateUserRequest, AdminUpdateUserResult, AdminUserDetail, AdminUserDetailAttachment,
    AdminUserDetailBankAccount, AdminUserDetailCompany, AdminUserDetailCountry,
    AdminUserDetailPosition, AdminUserDetailProfileImage, AdminUserDetailRoles,
    AdminUserDetailSalary, AdminUserDetailScope, AdminUserDetailUsername, AdminUserListPagingInput,
    AdminUserListPagingPage, DaySchedule, WeeklySchedule,
};
use kokkak_domain::traits::user::RepoError;
use kokkak_domain::UserRepository;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::auth::PasswordHasherPort;

/// Input for `AdminUserService::admin_insert_full`.
///
/// This is the application-layer DTO — handlers map their
/// wire-shape [`AdminInsertUserRequest`](crate::admin_user)
/// onto this struct (the handler is responsible for any
/// frontend-format normalization, like optional `Option<String>`
/// → typed fields).
///
/// Field names match the SP parameter names verbatim so the
/// repo layer can pass them through without rename.
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub struct AdminInsertUserFullInput {
    /// Caller-provided `user_guid` (the SP generates one when
    /// `None` / empty).
    pub user_guid: Option<String>,
    pub first_name: String,
    pub last_name: String,
    pub id_card: Option<String>,
    pub tel: Option<String>,
    pub email: String,
    pub gender: Option<String>,

    pub country_guid: Option<String>,
    pub province: Option<String>,
    pub district: Option<String>,
    pub sub_district: Option<String>,
    pub village: Option<String>,
    pub post: Option<String>,

    pub description: Option<String>,

    pub is_foreign: bool,
    pub is_customer_company: bool,
    pub is_customer: bool,
    pub is_admin: bool,
    pub is_employee: bool,
    pub is_freelance: bool,
    pub status: i32,

    pub username: String,
    /// **Plaintext** password — the service hashes it before
    /// hitting the SP. The handler never stores the plaintext.
    pub password: String,

    pub profile_img_path: Option<String>,

    pub company_guid: Option<String>,
    pub company_name: Option<String>,
    pub company_tel: Option<String>,
    pub company_type: Option<i32>,
    pub company_status: i32,

    pub department_guid: Option<String>,
    pub department_team_guid: Option<String>,

    pub position_guid: Option<String>,
    pub position_start_at: Option<DateTime<Utc>>,

    pub salary_amount: Option<Decimal>,
    pub salary_currency: Option<String>,

    pub schedule: WeeklySchedule,

    pub bank_name: Option<String>,
    pub bank_code: Option<String>,
    pub bank_account_no: Option<String>,
    pub bank_account_name: Option<String>,
    pub bank_book_img_path: Option<String>,

    pub id_card_front_path: Option<String>,
    pub id_card_back_path: Option<String>,
    pub proof_of_address_path: Option<String>,
    pub source_of_funds_statement_path: Option<String>,
}

/// Input for `AdminUserService::admin_update_full` (M22-b).
///
/// Mirrors [`AdminInsertUserFullInput`] 1:1 except:
/// - `user_guid` is **required** (the URL path carries it; the
///   service signature keeps it on the input struct so the
///   handler's DTO → input mapping stays flat).
/// - **No password field** — password reset is a separate flow.
///
/// Field names match the SP parameter names verbatim so the
/// repo layer can pass them through without rename.
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub struct AdminUpdateUserFullInput {
    /// The `[user].user_guid` to update.
    pub user_guid: String,
    pub first_name: String,
    pub last_name: String,
    pub id_card: Option<String>,
    pub tel: Option<String>,
    pub email: String,
    pub gender: Option<String>,

    pub country_guid: Option<String>,
    pub province: Option<String>,
    pub district: Option<String>,
    pub sub_district: Option<String>,
    pub village: Option<String>,
    pub post: Option<String>,

    pub description: Option<String>,

    pub is_foreign: bool,
    pub is_customer_company: bool,
    pub is_customer: bool,
    pub is_admin: bool,
    pub is_employee: bool,
    pub is_freelance: bool,
    pub status: i32,

    pub username: String,

    pub profile_img_path: Option<String>,

    pub company_guid: Option<String>,
    pub company_name: Option<String>,
    pub company_tel: Option<String>,
    pub company_type: Option<i32>,
    pub company_status: i32,

    pub department_guid: Option<String>,
    pub department_team_guid: Option<String>,

    pub position_guid: Option<String>,
    pub position_start_at: Option<DateTime<Utc>>,

    pub salary_amount: Option<Decimal>,
    pub salary_currency: Option<String>,

    pub schedule: WeeklySchedule,

    pub bank_name: Option<String>,
    pub bank_code: Option<String>,
    pub bank_account_no: Option<String>,
    pub bank_account_name: Option<String>,
    pub bank_book_img_path: Option<String>,

    pub id_card_front_path: Option<String>,
    pub id_card_back_path: Option<String>,
    pub proof_of_address_path: Option<String>,
    pub source_of_funds_statement_path: Option<String>,
}

/// Admin user use case bundle (M20-b).
///
/// Holds the user repository + password hasher. The hasher is
/// the same one [`crate::auth::AuthService`] uses, so the hash
/// format is identical between admin-provisioned and self-registered
/// accounts.
pub struct AdminUserService {
    users: Arc<dyn UserRepository>,
    hasher: Arc<dyn PasswordHasherPort>,
}

impl AdminUserService {
    /// Construct the service.
    pub fn new(users: Arc<dyn UserRepository>, hasher: Arc<dyn PasswordHasherPort>) -> Self {
        Self { users, hasher }
    }

    /// Admin-provision a new user via `SP_USER_INSERT_FULL`.
    ///
    /// `actor_user_guid` is the JWT's `user_guid` (NOT the
    /// `user_username_guid` the SP expects). The service resolves
    /// it before the SP call.
    ///
    /// On failure, returns the structured
    /// [`AdminInsertUserError`] with the SP's `code` + `message`.
    /// The handler maps `code` → HTTP status + `ErrorCode` string.
    pub async fn admin_insert_full(
        &self,
        actor_user_guid: Uuid,
        input: AdminInsertUserFullInput,
    ) -> Result<AdminInsertUserResult, AdminInsertUserError> {
        // 1. Resolve actor: JWT user_guid → user_username_guid.
        //
        // We short-circuit with the same codes the SP would emit
        // (ACTOR_NOT_FOUND) so the handler can use a single mapping
        // table. A suspended admin cannot impersonate because the
        // lookup filters `user_username_status <> 3`.
        let actor_user_username_guid = self
            .users
            .find_username_guid_by_user_guid(actor_user_guid)
            .await
            .map_err(|e| AdminInsertUserError::new("internal", format!("actor lookup: {e}")))?
            .ok_or_else(|| {
                AdminInsertUserError::new(
                    "actor_not_found",
                    "actor user_username_guid not found or inactive",
                )
            })?;

        // 2. Validate working schedule. The SP enforces this with a
        //    `WORK_TIME_REQUIRED` rejection; doing it client-side
        //    saves a DB round-trip on a common operator typo and
        //    lets us emit a precise 422 with a localized message
        //    pointing at the offending day.
        if let Err(day) = schedule_missing_times(&input.schedule) {
            return Err(AdminInsertUserError::new(
                "work_time_required",
                format!("working day must have start_time and end_time ({day})"),
            ));
        }

        // 3. Hash the password. The plaintext exists for the
        //    briefest possible moment — only inside this function
        //    scope.
        let password_hash = self.hasher.hash(&input.password).map_err(|e| {
            AdminInsertUserError::new("internal", format!("password hashing failed: {e}"))
        })?;

        // 4. Build the SP input + delegate to the repo.
        let req = AdminInsertUserRequest {
            actor_user_username_guid,
            user_guid: input.user_guid,
            first_name: input.first_name,
            last_name: input.last_name,
            id_card: input.id_card,
            tel: input.tel,
            email: input.email,
            gender: input.gender,
            country_guid: input.country_guid,
            province: input.province,
            district: input.district,
            sub_district: input.sub_district,
            village: input.village,
            post: input.post,
            description: input.description,
            is_foreign: input.is_foreign,
            is_customer_company: input.is_customer_company,
            is_customer: input.is_customer,
            is_admin: input.is_admin,
            is_employee: input.is_employee,
            is_freelance: input.is_freelance,
            status: if input.status == 0 { 0 } else { 1 },
            username: input.username,
            password_hash,
            profile_img_path: input.profile_img_path,
            company_guid: input.company_guid,
            company_name: input.company_name,
            company_tel: input.company_tel,
            company_type: input.company_type,
            company_status: if input.company_status == 0 { 0 } else { 1 },
            department_guid: input.department_guid,
            department_team_guid: input.department_team_guid,
            position_guid: input.position_guid,
            position_start_at: input.position_start_at,
            salary_amount: input.salary_amount,
            salary_currency: input.salary_currency,
            schedule: input.schedule,
            bank_name: input.bank_name,
            bank_code: input.bank_code,
            bank_account_no: input.bank_account_no,
            bank_account_name: input.bank_account_name,
            bank_book_img_path: input.bank_book_img_path,
            id_card_front_path: input.id_card_front_path,
            id_card_back_path: input.id_card_back_path,
            proof_of_address_path: input.proof_of_address_path,
            source_of_funds_statement_path: input.source_of_funds_statement_path,
        };

        self.users.admin_insert_full(&req).await
    }

    /// Admin user listing with page-based pagination (M21).
    ///
    /// Wraps `UserRepository::list_users_paging` (which calls
    /// `dbo.SP_USER_LIST_PAGING`). The actor is the JWT
    /// `user_guid` — forwarded for audit log consistency; the SP
    /// itself does NOT enforce admin gating (that lives at the
    /// handler layer via [`Permission::PageUsersView`]).
    ///
    /// ## Validation
    ///
    /// - `page < 1` → coerced to `1` (the SP does the same on its
    ///   side; we mirror it for a stable wire contract).
    /// - `page_size` clamped to `1..=100` (matches the SP's own cap,
    ///   also matches the M16 admin list endpoint's cap).
    /// - `keyword` is trimmed; an all-whitespace string collapses
    ///   to empty so the SP's `LIKE N'%%'` path runs (no filter).
    ///
    /// - `user_status` is forwarded verbatim — the SP accepts `NULL`
    ///   to mean "all statuses".
    /// - `user_is_customer` / `user_is_employee` / `user_is_freelance`
    ///   are forwarded verbatim — `Some(true)` filters to that cohort,
    ///   `Some(false)` filters to the opposite, `None` means "no
    ///   filter" (the SP receives `NULL`).
    /// - `department_guid` / `department_team_guid` / `position_guid`
    ///   are trimmed; an all-whitespace string collapses to `None`
    ///   so the SP's `= ''` short-circuit runs (no filter).
    ///
    /// ponytail: clamping lives here AND in the SP — the application
    /// layer's clamp guarantees the wire shape (`page_size` is
    /// always 1..=100, `page` is always ≥ 1) so the frontend never
    /// has to special-case the response. The SP's own clamp is the
    /// defense-in-depth backup.
    pub async fn list_users_paging(
        &self,
        actor: Uuid,
        input: AdminUserListPagingInput,
    ) -> Result<AdminUserListPagingPage, RepoError> {
        // Normalize inputs — keep the wire contract predictable.
        //
        // The three GUID scope filters are stored as trimmed
        // `Option<String>` so the SP's `= ''` short-circuit works
        // when the caller sends an all-whitespace value. The three
        // bit filters and `user_status` are forwarded as-is — the
        // SP already understands `NULL` to mean "no filter" on
        // every one of them.
        let trim_to_none = |s: Option<String>| -> Option<String> {
            s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
        };
        let normalized = AdminUserListPagingInput {
            keyword: input.keyword.trim().to_string(),
            user_status: input.user_status,
            user_is_customer: input.user_is_customer,
            user_is_employee: input.user_is_employee,
            user_is_freelance: input.user_is_freelance,
            department_guid: trim_to_none(input.department_guid),
            department_team_guid: trim_to_none(input.department_team_guid),
            position_guid: trim_to_none(input.position_guid),
            page: input.page.max(1),
            page_size: input.page_size.clamp(1, 100),
        };

        let mut page = self.users.list_users_paging(&normalized, actor).await?;

        // Mirror the normalized page / page_size back onto the
        // envelope so the frontend sees the same numbers we sent
        // down (instead of whatever the SP echoed).
        page.page = normalized.page as i32;
        page.page_size = normalized.page_size as i32;
        Ok(page)
    }

    /// M22: full detail lookup for a single user — wraps
    /// `dbo.SP_USER_DETAIL_FULL_GET`.
    ///
    /// Read-side counterpart to [`Self::admin_insert_full`]. The
    /// service is intentionally thin: it just forwards the GUID to
    /// the repo + forwards the actor for audit log consistency.
    /// All field-level validation (GUID format, soft-delete
    /// handling) lives in the SP + repo layer.
    ///
    /// `actor_user_guid` is forwarded for audit logging; the SP
    /// itself does NOT enforce admin gating (the handler already
    /// gates on `Permission::PageUsersView`).
    ///
    /// Returns `Ok(None)` when the user doesn't resolve or is
    /// soft-deleted; the handler maps that to a 404 `not_found`.
    pub async fn get_user_detail_full(
        &self,
        actor_user_guid: Uuid,
        user_guid: Uuid,
    ) -> Result<Option<AdminUserDetail>, RepoError> {
        self.users
            .get_user_detail_full(user_guid, actor_user_guid)
            .await
    }

    /// M22-b: admin user update via `SP_USER_UPDATE_FULL`.
    ///
    /// Write-side counterpart to [`Self::admin_insert_full`].
    /// Updates the matching `[user]` row + linked
    /// `[user_username]` row in one transaction.
    ///
    /// ## Flow
    ///
    /// 1. **Resolve actor**: `find_username_guid_by_user_guid(jwt.id)`
    ///    maps the JWT's `user_guid` → the SP's required
    ///    `user_username_guid`. Returns [`AdminUpdateUserError`]
    ///    with `actor_not_found` if the admin is missing / suspended.
    /// 2. **Validate working schedule**: same
    ///    `WORK_TIME_REQUIRED` short-circuit as the insert flow.
    /// 3. **Build** [`AdminUpdateUserRequest`] and delegate to
    ///    [`UserRepository::admin_update_full`]. No password hashing
    ///    — the SP never sees the password column on update.
    ///
    /// `actor_user_guid` is the JWT's `user_guid`. On failure,
    /// returns the structured [`AdminUpdateUserError`] with the
    /// SP's `code` + `message`. The handler maps `code` → HTTP
    /// status + `ErrorCode` string.
    pub async fn admin_update_full(
        &self,
        actor_user_guid: Uuid,
        input: AdminUpdateUserFullInput,
    ) -> Result<AdminUpdateUserResult, AdminUpdateUserError> {
        // 1. Resolve actor: JWT user_guid → user_username_guid.
        //
        // Mirrors the insert flow. We short-circuit with the same
        // codes the SP would emit (ACTOR_NOT_FOUND) so the handler
        // can use a single mapping table.
        let actor_user_username_guid = self
            .users
            .find_username_guid_by_user_guid(actor_user_guid)
            .await
            .map_err(|e| AdminUpdateUserError::new("internal", format!("actor lookup: {e}")))?
            .ok_or_else(|| {
                AdminUpdateUserError::new(
                    "actor_not_found",
                    "actor user_username_guid not found or inactive",
                )
            })?;

        // 2. Validate working schedule. Same check as insert —
        //    short-circuits the SP round-trip on a common
        //    operator typo.
        if let Err(day) = schedule_missing_times(&input.schedule) {
            return Err(AdminUpdateUserError::new(
                "work_time_required",
                format!("working day must have start_time and end_time ({day})"),
            ));
        }

        // 3. Build the SP input + delegate to the repo. No password
        //    hashing — the update SP doesn't touch the column.
        let req = AdminUpdateUserRequest {
            actor_user_username_guid,
            user_guid: input.user_guid,
            first_name: input.first_name,
            last_name: input.last_name,
            id_card: input.id_card,
            tel: input.tel,
            email: input.email,
            gender: input.gender,
            country_guid: input.country_guid,
            province: input.province,
            district: input.district,
            sub_district: input.sub_district,
            village: input.village,
            post: input.post,
            description: input.description,
            is_foreign: input.is_foreign,
            is_customer_company: input.is_customer_company,
            is_customer: input.is_customer,
            is_admin: input.is_admin,
            is_employee: input.is_employee,
            is_freelance: input.is_freelance,
            status: if input.status == 0 { 0 } else { 1 },
            username: input.username,
            profile_img_path: input.profile_img_path,
            company_guid: input.company_guid,
            company_name: input.company_name,
            company_tel: input.company_tel,
            company_type: input.company_type,
            company_status: if input.company_status == 0 { 0 } else { 1 },
            department_guid: input.department_guid,
            department_team_guid: input.department_team_guid,
            position_guid: input.position_guid,
            position_start_at: input.position_start_at,
            salary_amount: input.salary_amount,
            salary_currency: input.salary_currency,
            schedule: input.schedule,
            bank_name: input.bank_name,
            bank_code: input.bank_code,
            bank_account_no: input.bank_account_no,
            bank_account_name: input.bank_account_name,
            bank_book_img_path: input.bank_book_img_path,
            id_card_front_path: input.id_card_front_path,
            id_card_back_path: input.id_card_back_path,
            proof_of_address_path: input.proof_of_address_path,
            source_of_funds_statement_path: input.source_of_funds_statement_path,
        };

        self.users.admin_update_full(&req).await
    }
}

/// Check every day in the weekly schedule: when `is_working = 1`,
/// both `start_time` and `end_time` must be `Some(_)`.
///
/// Returns `Err(day_label)` on the first offender (the handler
/// surfaces the day name in the localized message); `Ok(())`
/// when the schedule is valid.
fn schedule_missing_times(s: &WeeklySchedule) -> Result<(), &'static str> {
    check_day("monday", &s.monday)?;
    check_day("tuesday", &s.tuesday)?;
    check_day("wednesday", &s.wednesday)?;
    check_day("thursday", &s.thursday)?;
    check_day("friday", &s.friday)?;
    check_day("saturday", &s.saturday)?;
    check_day("sunday", &s.sunday)?;
    Ok(())
}

fn check_day(label: &'static str, d: &DaySchedule) -> Result<(), &'static str> {
    if d.is_working && (d.start_time.is_none() || d.end_time.is_none()) {
        return Err(label);
    }
    Ok(())
}

// Silence unused import warning for `RepoError` (kept for future
// error mapping; the trait method today returns the structured
// `AdminInsertUserError` instead).
#[allow(dead_code)]
const _REPO_ERROR_TOUCH: fn(RepoError) = |_| {};

#[cfg(test)]
mod tests {
    //! Unit tests for the Rust-side validation. Integration coverage
    //! of the full SP call lives in the `tests/` integration suite
    //! once a SQL Server test container is wired up.
    use super::*;
    use kokkak_domain::admin_user::DaySchedule;

    fn ws(days: &[DaySchedule; 7]) -> WeeklySchedule {
        WeeklySchedule {
            monday: days[0].clone(),
            tuesday: days[1].clone(),
            wednesday: days[2].clone(),
            thursday: days[3].clone(),
            friday: days[4].clone(),
            saturday: days[5].clone(),
            sunday: days[6].clone(),
        }
    }

    fn off() -> DaySchedule {
        DaySchedule {
            is_working: false,
            start_time: None,
            end_time: None,
        }
    }

    fn on(t: &str) -> DaySchedule {
        // Parse "HH:MM:SS" string into NaiveTime. The test code
        // uses chrono's default format ("%H:%M:%S") which matches
        // the SQL Server `time(0)` wire format.
        let parsed = t.parse::<chrono::NaiveTime>().expect("test time parse");
        DaySchedule {
            is_working: true,
            start_time: Some(parsed),
            end_time: Some(parsed),
        }
    }

    #[test]
    fn schedule_all_off_passes() {
        assert!(
            schedule_missing_times(&ws(&[off(), off(), off(), off(), off(), off(), off()])).is_ok()
        );
    }

    #[test]
    fn schedule_all_on_with_times_passes() {
        let days = [
            on("09:00:00"),
            on("09:00:00"),
            on("09:00:00"),
            on("09:00:00"),
            on("09:00:00"),
            on("10:00:00"),
            on("10:00:00"),
        ];
        assert!(schedule_missing_times(&ws(&days)).is_ok());
    }

    #[test]
    fn schedule_on_without_start_fails() {
        let bad = DaySchedule {
            is_working: true,
            start_time: None,
            end_time: Some("17:00:00".parse::<chrono::NaiveTime>().unwrap()),
        };
        let days = [off(), bad.clone(), off(), off(), off(), off(), off()];
        let err = schedule_missing_times(&ws(&days)).unwrap_err();
        assert_eq!(err, "tuesday");
    }

    #[test]
    fn schedule_on_without_end_fails() {
        let bad = DaySchedule {
            is_working: true,
            start_time: Some("09:00:00".parse::<chrono::NaiveTime>().unwrap()),
            end_time: None,
        };
        let days = [off(), off(), off(), off(), bad.clone(), off(), off()];
        let err = schedule_missing_times(&ws(&days)).unwrap_err();
        assert_eq!(err, "friday");
    }

    // ----- M21: list_users_paging normalization tests -----

    use std::sync::Arc;

    use kokkak_domain::{
        admin_user::{AdminUserListPagingPage, UserListPagingRow},
        RepoError, UserRepository,
    };

    /// In-memory mock that records the last `list_users_paging` input
    /// and returns a canned page. All other trait methods are
    /// stubs that return `Backend` — they're not exercised by these
    /// tests.
    #[derive(Default)]
    struct RecordingRepo {
        last_input: std::sync::Mutex<Option<AdminUserListPagingInput>>,
    }

    #[async_trait::async_trait]
    impl UserRepository for RecordingRepo {
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<kokkak_domain::User>, RepoError> {
            Err(RepoError::Backend("recording mock: find_by_id".into()))
        }
        async fn find_by_username(
            &self,
            _u: &str,
        ) -> Result<Option<kokkak_domain::User>, RepoError> {
            Err(RepoError::Backend(
                "recording mock: find_by_username".into(),
            ))
        }
        async fn insert(&self, _u: &kokkak_domain::User) -> Result<(), RepoError> {
            Err(RepoError::Backend("recording mock: insert".into()))
        }
        async fn update(&self, _u: &kokkak_domain::User) -> Result<(), RepoError> {
            Err(RepoError::Backend("recording mock: update".into()))
        }
        async fn list_with_permissions(
            &self,
            _caller: Uuid,
        ) -> Result<Vec<kokkak_domain::UserListRow>, RepoError> {
            Err(RepoError::Backend(
                "recording mock: list_with_permissions".into(),
            ))
        }
        async fn find_username_guid_by_user_guid(
            &self,
            _id: Uuid,
        ) -> Result<Option<String>, RepoError> {
            Err(RepoError::Backend(
                "recording mock: find_username_guid_by_user_guid".into(),
            ))
        }
        async fn admin_insert_full(
            &self,
            _req: &kokkak_domain::AdminInsertUserRequest,
        ) -> Result<kokkak_domain::AdminInsertUserResult, kokkak_domain::AdminInsertUserError>
        {
            Err(kokkak_domain::AdminInsertUserError::new(
                "internal",
                "recording mock: admin_insert_full",
            ))
        }
        async fn list_users_paging(
            &self,
            input: &AdminUserListPagingInput,
            _actor: Uuid,
        ) -> Result<AdminUserListPagingPage, RepoError> {
            *self.last_input.lock().unwrap() = Some(input.clone());
            Ok(AdminUserListPagingPage {
                items: vec![UserListPagingRow {
                    user_guid: Uuid::new_v4().to_string(),
                    ..Default::default()
                }],
                total_count: 1,
                page: input.page as i32,
                page_size: input.page_size as i32,
            })
        }
        async fn get_user_detail_full(
            &self,
            _user_guid: Uuid,
            _actor: Uuid,
        ) -> Result<Option<AdminUserDetail>, RepoError> {
            Err(RepoError::Backend(
                "recording mock: get_user_detail_full".into(),
            ))
        }
    }

    /// AdminUserService only needs a repo for `list_users_paging`; the
    /// password hasher is unused for this method so we pass `None` is
    /// impossible — build a no-op hasher instead.
    fn svc_with(repo: Arc<dyn UserRepository>) -> AdminUserService {
        // The password hasher isn't called by list_users_paging, but
        // `AdminUserService::new` requires one. Use a never-invoked
        // closure-style adapter via the PasswordHasherPort trait.
        use crate::auth::PasswordHasherPort;
        struct NoopHasher;
        impl PasswordHasherPort for NoopHasher {
            fn hash(&self, _plain: &str) -> Result<String, kokkak_domain::AuthError> {
                Err(kokkak_domain::AuthError::Backend(
                    "noop hasher: not used in list_users_paging tests".into(),
                ))
            }
            fn verify(&self, _plain: &str, _hash: &str) -> Result<(), kokkak_domain::AuthError> {
                Err(kokkak_domain::AuthError::Backend("noop".into()))
            }
            fn dummy_hash(&self) -> &str {
                // Tests never call verify / dummy_hash — only
                // `list_users_paging` is exercised. Return a
                // syntactically-valid argon2id PHC string to satisfy
                // the port contract without inventing a path.
                "$argon2id$v=19$m=19456,t=2,p=1$YWFhYWFhYWFhYWFh$YWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWE"
            }
        }
        AdminUserService::new(repo, Arc::new(NoopHasher))
    }

    #[tokio::test]
    async fn list_users_paging_clamps_page_size_to_100() {
        let repo = Arc::new(RecordingRepo::default());
        let svc = svc_with(repo.clone());
        let actor = Uuid::new_v4();

        let page = svc
            .list_users_paging(
                actor,
                AdminUserListPagingInput {
                    keyword: "x".into(),
                    page: 1,
                    page_size: 9999, // way over the cap
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        // Service clamps page_size to 100.
        assert_eq!(page.page_size, 100);
        // Forwarded to the repo with the clamped value.
        assert_eq!(
            repo.last_input.lock().unwrap().as_ref().unwrap().page_size,
            100
        );
    }

    #[tokio::test]
    async fn list_users_paging_clamps_page_to_at_least_one() {
        let repo = Arc::new(RecordingRepo::default());
        let svc = svc_with(repo.clone());
        let actor = Uuid::new_v4();

        let page = svc
            .list_users_paging(
                actor,
                AdminUserListPagingInput {
                    keyword: String::new(),
                    page: 0, // < 1 should clamp to 1
                    page_size: 20,
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        assert_eq!(page.page, 1);
        assert_eq!(repo.last_input.lock().unwrap().as_ref().unwrap().page, 1);
    }

    #[tokio::test]
    async fn list_users_paging_trims_whitespace_keyword() {
        let repo = Arc::new(RecordingRepo::default());
        let svc = svc_with(repo.clone());
        let actor = Uuid::new_v4();

        svc.list_users_paging(
            actor,
            AdminUserListPagingInput {
                keyword: "   somchai   ".into(),
                page: 1,
                page_size: 20,
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let sent = repo.last_input.lock().unwrap();
        assert_eq!(sent.as_ref().unwrap().keyword, "somchai");
    }

    #[tokio::test]
    async fn list_users_paging_clamps_page_size_lower_bound_to_one() {
        let repo = Arc::new(RecordingRepo::default());
        let svc = svc_with(repo.clone());
        let actor = Uuid::new_v4();

        let page = svc
            .list_users_paging(
                actor,
                AdminUserListPagingInput {
                    keyword: String::new(),
                    page: 1,
                    page_size: 0,
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        // page_size=0 → clamped to 1 (don't allow empty pages).
        assert_eq!(page.page_size, 1);
    }

    // ----- New: scope-GUID filters collapse whitespace to None -----

    #[tokio::test]
    async fn list_users_paging_collapses_whitespace_scope_guids_to_none() {
        let repo = Arc::new(RecordingRepo::default());
        let svc = svc_with(repo.clone());
        let actor = Uuid::new_v4();

        svc.list_users_paging(
            actor,
            AdminUserListPagingInput {
                keyword: String::new(),
                page: 1,
                page_size: 20,
                department_guid: Some("   ".into()),
                department_team_guid: Some("\t".into()),
                position_guid: Some(" ".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let sent = repo.last_input.lock().unwrap();
        let sent = sent.as_ref().unwrap();
        // All-whitespace scope filters are collapsed to None so
        // the SP's `= ''` short-circuit runs (no filter).
        assert!(sent.department_guid.is_none());
        assert!(sent.department_team_guid.is_none());
        assert!(sent.position_guid.is_none());
    }

    #[tokio::test]
    async fn list_users_paging_preserves_non_empty_scope_guids() {
        let repo = Arc::new(RecordingRepo::default());
        let svc = svc_with(repo.clone());
        let actor = Uuid::new_v4();

        svc.list_users_paging(
            actor,
            AdminUserListPagingInput {
                keyword: String::new(),
                page: 1,
                page_size: 20,
                department_guid: Some("  dept-abc  ".into()),
                department_team_guid: Some("team-xyz".into()),
                position_guid: Some("  pos-1".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let sent = repo.last_input.lock().unwrap();
        let sent = sent.as_ref().unwrap();
        // Non-empty values pass through (trimmed).
        assert_eq!(sent.department_guid.as_deref(), Some("dept-abc"));
        assert_eq!(sent.department_team_guid.as_deref(), Some("team-xyz"));
        assert_eq!(sent.position_guid.as_deref(), Some("pos-1"));
    }

    // ----- M22: get_user_detail_full tests -----

    use std::sync::atomic::{AtomicU32, Ordering};

    use kokkak_domain::admin_user::AdminUserDetail;

    /// In-memory mock that records every `get_user_detail_full`
    /// call. The `scripted_outcome` field drives the response:
    /// `0` → `Ok(Some(detail))`, `1` → `Ok(None)` (not found),
    /// anything else → `Err(Backend(...))`.
    #[derive(Default)]
    struct DetailMock {
        call_count: AtomicU32,
        last_user_guid: std::sync::Mutex<Option<Uuid>>,
        last_actor: std::sync::Mutex<Option<Uuid>>,
        scripted_outcome: AtomicU32,
    }

    #[async_trait::async_trait]
    impl UserRepository for DetailMock {
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<kokkak_domain::User>, RepoError> {
            Err(RepoError::Backend("detail mock: find_by_id".into()))
        }
        async fn find_by_username(
            &self,
            _u: &str,
        ) -> Result<Option<kokkak_domain::User>, RepoError> {
            Err(RepoError::Backend("detail mock: find_by_username".into()))
        }
        async fn insert(&self, _u: &kokkak_domain::User) -> Result<(), RepoError> {
            Err(RepoError::Backend("detail mock: insert".into()))
        }
        async fn update(&self, _u: &kokkak_domain::User) -> Result<(), RepoError> {
            Err(RepoError::Backend("detail mock: update".into()))
        }
        async fn list_with_permissions(
            &self,
            _caller: Uuid,
        ) -> Result<Vec<kokkak_domain::UserListRow>, RepoError> {
            Err(RepoError::Backend(
                "detail mock: list_with_permissions".into(),
            ))
        }
        async fn find_username_guid_by_user_guid(
            &self,
            _id: Uuid,
        ) -> Result<Option<String>, RepoError> {
            Err(RepoError::Backend(
                "detail mock: find_username_guid_by_user_guid".into(),
            ))
        }
        async fn admin_insert_full(
            &self,
            _req: &kokkak_domain::AdminInsertUserRequest,
        ) -> Result<kokkak_domain::AdminInsertUserResult, kokkak_domain::AdminInsertUserError>
        {
            Err(kokkak_domain::AdminInsertUserError::new(
                "internal",
                "detail mock: admin_insert_full",
            ))
        }
        async fn list_users_paging(
            &self,
            _input: &AdminUserListPagingInput,
            _actor: Uuid,
        ) -> Result<AdminUserListPagingPage, RepoError> {
            Err(RepoError::Backend("detail mock: list_users_paging".into()))
        }
        async fn get_user_detail_full(
            &self,
            user_guid: Uuid,
            actor: Uuid,
        ) -> Result<Option<AdminUserDetail>, RepoError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            *self.last_user_guid.lock().unwrap() = Some(user_guid);
            *self.last_actor.lock().unwrap() = Some(actor);
            match self.scripted_outcome.load(Ordering::SeqCst) {
                0 => Ok(Some(AdminUserDetail {
                    user_guid: user_guid.to_string(),
                    user_first_name: "Anousith".into(),
                    user_last_name: "Tester".into(),
                    full_name: "Anousith Tester".into(),
                    user_status: 1,
                    user_status_name: "Active".into(),
                    ..Default::default()
                })),
                1 => Ok(None),
                _ => Err(RepoError::Backend(
                    "detail mock: simulated backend failure".into(),
                )),
            }
        }
    }

    #[tokio::test]
    async fn get_user_detail_full_returns_some_when_repo_finds_user() {
        let repo = Arc::new(DetailMock::default());
        let svc = svc_with(repo.clone());
        let actor = Uuid::new_v4();
        let target = Uuid::new_v4();

        let detail = svc.get_user_detail_full(actor, target).await.unwrap();

        // Service forwards the result verbatim — `Some(detail)` on
        // a successful repo lookup.
        let detail = detail.expect("repo returned Some(detail)");
        assert_eq!(detail.user_guid, target.to_string());
        assert_eq!(detail.user_first_name, "Anousith");
        assert_eq!(detail.user_status, 1);
        assert_eq!(detail.user_status_name, "Active");

        // The mock recorded exactly one call + the right args.
        assert_eq!(repo.call_count.load(Ordering::SeqCst), 1);
        assert_eq!(*repo.last_user_guid.lock().unwrap(), Some(target));
        assert_eq!(*repo.last_actor.lock().unwrap(), Some(actor));
    }

    #[tokio::test]
    async fn get_user_detail_full_returns_none_when_repo_finds_nothing() {
        let repo = Arc::new(DetailMock::default());
        repo.scripted_outcome.store(1, Ordering::SeqCst);
        let svc = svc_with(repo.clone());
        let actor = Uuid::new_v4();
        let target = Uuid::new_v4();

        let detail = svc.get_user_detail_full(actor, target).await.unwrap();

        // The handler maps `Ok(None)` to a 404 `not_found` envelope.
        assert!(detail.is_none(), "expected None (handler will 404)");
    }

    #[tokio::test]
    async fn get_user_detail_full_propagates_repo_error() {
        let repo = Arc::new(DetailMock::default());
        repo.scripted_outcome.store(2, Ordering::SeqCst);
        let svc = svc_with(repo.clone());
        let actor = Uuid::new_v4();
        let target = Uuid::new_v4();

        // The handler maps any `RepoError` (other than `NotFound`)
        // to a 500 `internal` envelope via `into_localized_response`.
        let err = svc
            .get_user_detail_full(actor, target)
            .await
            .expect_err("expected Backend error");
        assert!(matches!(err, RepoError::Backend(_)));
    }

    #[tokio::test]
    async fn get_user_detail_full_forwards_actor_unchanged() {
        // The trait signature carries `actor` for future audit-log
        // SP extensions; today the service must pass the actor
        // through to the repo without renaming. This test guards
        // that contract — if a refactor swaps the actor for the
        // target (or vice versa), this test fails loudly.
        let repo = Arc::new(DetailMock::default());
        let svc = svc_with(repo.clone());
        let actor = Uuid::new_v4();
        let target = Uuid::new_v4();

        svc.get_user_detail_full(actor, target).await.unwrap();

        assert_eq!(*repo.last_actor.lock().unwrap(), Some(actor));
        assert_eq!(*repo.last_user_guid.lock().unwrap(), Some(target));
    }

    // ----- M22-b: admin_update_full service tests -----

    /// Mock that records the last `admin_update_full` call and
    /// returns a scripted outcome (success / not-found /
    /// backend-error). Mirrors `DetailMock` but for the update
    /// trait method.
    #[derive(Default)]
    struct UpdateMock {
        last_input: std::sync::Mutex<Option<AdminUpdateUserRequest>>,
        scripted_outcome: std::sync::atomic::AtomicU8,
    }

    #[async_trait::async_trait]
    impl UserRepository for UpdateMock {
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<kokkak_domain::User>, RepoError> {
            Err(RepoError::Backend("update mock: find_by_id".into()))
        }
        async fn find_by_username(
            &self,
            _u: &str,
        ) -> Result<Option<kokkak_domain::User>, RepoError> {
            Err(RepoError::Backend("update mock: find_by_username".into()))
        }
        async fn insert(&self, _u: &kokkak_domain::User) -> Result<(), RepoError> {
            Err(RepoError::Backend("update mock: insert".into()))
        }
        async fn update(&self, _u: &kokkak_domain::User) -> Result<(), RepoError> {
            Err(RepoError::Backend("update mock: update".into()))
        }
        async fn list_with_permissions(
            &self,
            _caller: Uuid,
        ) -> Result<Vec<kokkak_domain::UserListRow>, RepoError> {
            Err(RepoError::Backend(
                "update mock: list_with_permissions".into(),
            ))
        }
        async fn find_username_guid_by_user_guid(
            &self,
            _id: Uuid,
        ) -> Result<Option<String>, RepoError> {
            // The actor's `user_username_guid` — the update flow
            // needs this to pass to the SP. Return a stable sentinel.
            Ok(Some(Uuid::nil().to_string()))
        }
        async fn admin_insert_full(
            &self,
            _req: &kokkak_domain::AdminInsertUserRequest,
        ) -> Result<kokkak_domain::AdminInsertUserResult, kokkak_domain::AdminInsertUserError>
        {
            Err(kokkak_domain::AdminInsertUserError::new(
                "internal",
                "update mock: admin_insert_full",
            ))
        }
        async fn list_users_paging(
            &self,
            _input: &AdminUserListPagingInput,
            _actor: Uuid,
        ) -> Result<AdminUserListPagingPage, RepoError> {
            Err(RepoError::Backend("update mock: list_users_paging".into()))
        }
        async fn get_user_detail_full(
            &self,
            _user_guid: Uuid,
            _actor: Uuid,
        ) -> Result<Option<AdminUserDetail>, RepoError> {
            Err(RepoError::Backend(
                "update mock: get_user_detail_full".into(),
            ))
        }
        async fn admin_update_full(
            &self,
            req: &kokkak_domain::AdminUpdateUserRequest,
        ) -> Result<kokkak_domain::AdminUpdateUserResult, kokkak_domain::AdminUpdateUserError>
        {
            use std::sync::atomic::Ordering;
            *self.last_input.lock().unwrap() = Some(req.clone());
            match self.scripted_outcome.load(Ordering::SeqCst) {
                0 => Ok(kokkak_domain::AdminUpdateUserResult {
                    user_guid: req.user_guid.clone(),
                }),
                1 => Err(kokkak_domain::AdminUpdateUserError::new(
                    "user_not_found",
                    "user not found",
                )),
                _ => Err(kokkak_domain::AdminUpdateUserError::new(
                    "internal",
                    "update mock: simulated backend failure",
                )),
            }
        }
    }

    fn update_input(target_guid: &str) -> AdminUpdateUserFullInput {
        AdminUpdateUserFullInput {
            user_guid: target_guid.to_string(),
            first_name: "Anousith".into(),
            last_name: "Updated".into(),
            id_card: Some("12345".into()),
            tel: Some("+85620".into()),
            email: "anousith@kokkak.com".into(),
            gender: Some("male".into()),
            country_guid: None,
            province: None,
            district: None,
            sub_district: None,
            village: None,
            post: None,
            description: Some("unit test update".into()),
            is_foreign: false,
            is_customer_company: false,
            is_customer: false,
            is_admin: false,
            is_employee: true,
            is_freelance: false,
            status: 1,
            username: "anousith".into(),
            profile_img_path: None,
            company_guid: None,
            company_name: None,
            company_tel: None,
            company_type: None,
            company_status: 1,
            department_guid: None,
            department_team_guid: None,
            position_guid: None,
            position_start_at: None,
            salary_amount: None,
            salary_currency: Some("THB".into()),
            schedule: WeeklySchedule::default(),
            bank_name: None,
            bank_code: None,
            bank_account_no: None,
            bank_account_name: None,
            bank_book_img_path: None,
            id_card_front_path: None,
            id_card_back_path: None,
            proof_of_address_path: None,
            source_of_funds_statement_path: None,
        }
    }

    #[tokio::test]
    async fn admin_update_full_returns_repo_result_when_actor_resolves() {
        let repo = Arc::new(UpdateMock::default());
        let svc = svc_with(repo.clone());
        let actor = Uuid::new_v4();
        let target_guid = Uuid::new_v4().to_string();

        let result = svc
            .admin_update_full(actor, update_input(&target_guid))
            .await
            .expect("update should succeed when repo is scripted ok");

        assert_eq!(result.user_guid, target_guid);

        // Service must have forwarded the input verbatim to the repo.
        let recorded = repo
            .last_input
            .lock()
            .unwrap()
            .clone()
            .expect("repo called");
        assert_eq!(recorded.user_guid, target_guid);
        assert_eq!(recorded.first_name, "Anousith");
        assert_eq!(recorded.last_name, "Updated");
        // No password_hash field on the update request — the
        // update SP doesn't touch the column.
        assert_eq!(recorded.username, "anousith");
    }

    #[tokio::test]
    async fn admin_update_full_returns_actor_not_found_when_admin_lookup_fails() {
        // A repo that returns `None` from
        // `find_username_guid_by_user_guid` simulates a missing /
        // suspended admin. The service must short-circuit with
        // `actor_not_found` before the SP call — mirrors the
        // insert flow's behavior.
        struct MissingActorRepo;
        #[async_trait::async_trait]
        impl UserRepository for MissingActorRepo {
            async fn find_username_guid_by_user_guid(
                &self,
                _id: Uuid,
            ) -> Result<Option<String>, RepoError> {
                Ok(None)
            }
            async fn admin_update_full(
                &self,
                _req: &kokkak_domain::AdminUpdateUserRequest,
            ) -> Result<kokkak_domain::AdminUpdateUserResult, kokkak_domain::AdminUpdateUserError>
            {
                panic!("repo should not be reached when actor is missing")
            }
            // Other methods unused by this test path.
            async fn find_by_id(&self, _: Uuid) -> Result<Option<kokkak_domain::User>, RepoError> {
                Err(RepoError::Backend("unused".into()))
            }
            async fn find_by_username(
                &self,
                _: &str,
            ) -> Result<Option<kokkak_domain::User>, RepoError> {
                Err(RepoError::Backend("unused".into()))
            }
            async fn insert(&self, _: &kokkak_domain::User) -> Result<(), RepoError> {
                Err(RepoError::Backend("unused".into()))
            }
            async fn update(&self, _: &kokkak_domain::User) -> Result<(), RepoError> {
                Err(RepoError::Backend("unused".into()))
            }
            async fn list_with_permissions(
                &self,
                _: Uuid,
            ) -> Result<Vec<kokkak_domain::UserListRow>, RepoError> {
                Err(RepoError::Backend("unused".into()))
            }
            async fn admin_insert_full(
                &self,
                _: &kokkak_domain::AdminInsertUserRequest,
            ) -> Result<kokkak_domain::AdminInsertUserResult, kokkak_domain::AdminInsertUserError>
            {
                Err(kokkak_domain::AdminInsertUserError::new(
                    "internal", "unused",
                ))
            }
            async fn list_users_paging(
                &self,
                _: &AdminUserListPagingInput,
                _: Uuid,
            ) -> Result<AdminUserListPagingPage, RepoError> {
                Err(RepoError::Backend("unused".into()))
            }
            async fn get_user_detail_full(
                &self,
                _: Uuid,
                _: Uuid,
            ) -> Result<Option<AdminUserDetail>, RepoError> {
                Err(RepoError::Backend("unused".into()))
            }
        }
        let svc = AdminUserService::new(
            Arc::new(MissingActorRepo),
            // PasswordHasherPort is required by `new` but unused here.
            Arc::new(NoopHasher),
        );

        let err = svc
            .admin_update_full(Uuid::new_v4(), update_input(&Uuid::new_v4().to_string()))
            .await
            .expect_err("actor lookup failure must short-circuit");
        assert_eq!(err.code, "actor_not_found");
    }

    #[tokio::test]
    async fn admin_update_full_short_circuits_on_invalid_schedule() {
        // A working day with `is_working = true` but missing
        // times must short-circuit with `work_time_required`
        // before the SP call — same contract as the insert
        // flow.
        let repo = Arc::new(UpdateMock::default());
        let svc = svc_with(repo.clone());

        let bad_day = DaySchedule {
            is_working: true,
            start_time: None,
            end_time: Some("17:00:00".parse::<chrono::NaiveTime>().unwrap()),
        };
        let days = [bad_day.clone(), off(), off(), off(), off(), off(), off()];
        let schedule = WeeklySchedule {
            monday: days[0].clone(),
            tuesday: days[1].clone(),
            wednesday: days[2].clone(),
            thursday: days[3].clone(),
            friday: days[4].clone(),
            saturday: days[5].clone(),
            sunday: days[6].clone(),
        };

        let mut input = update_input(&Uuid::new_v4().to_string());
        input.schedule = schedule;

        let err = svc
            .admin_update_full(Uuid::new_v4(), input)
            .await
            .expect_err("missing work times must fail");
        assert_eq!(err.code, "work_time_required");

        // Repo must NOT have been called — the validation
        // short-circuit fires before the SP call.
        assert!(
            repo.last_input.lock().unwrap().is_none(),
            "repo must not be reached when schedule is invalid"
        );
    }

    #[tokio::test]
    async fn admin_update_full_propagates_user_not_found_from_repo() {
        let repo = Arc::new(UpdateMock::default());
        repo.scripted_outcome
            .store(1, std::sync::atomic::Ordering::SeqCst);
        let svc = svc_with(repo);

        let err = svc
            .admin_update_full(Uuid::new_v4(), update_input(&Uuid::new_v4().to_string()))
            .await
            .expect_err("scripted user_not_found must surface");
        assert_eq!(err.code, "user_not_found");
    }

    /// Noop hasher used by the update tests — same as
    /// `svc_with` defines locally. Re-declared here so the
    /// test closure above can construct a service directly
    /// with a non-default repo.
    struct NoopHasher;
    impl crate::auth::PasswordHasherPort for NoopHasher {
        fn hash(&self, _plain: &str) -> Result<String, kokkak_domain::AuthError> {
            Err(kokkak_domain::AuthError::Backend("noop".into()))
        }
        fn verify(&self, _plain: &str, _hash: &str) -> Result<(), kokkak_domain::AuthError> {
            Err(kokkak_domain::AuthError::Backend("noop".into()))
        }
        fn dummy_hash(&self) -> &str {
            "$argon2id$v=19$m=19456,t=2,p=1$YWFhYWFhYWFhYWFh$YWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWE"
        }
    }
}
