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

use std::sync::Arc;

use chrono::{DateTime, Utc};
use kokkak_domain::admin_user::{
    AdminInsertUserError, AdminInsertUserRequest, AdminInsertUserResult, DaySchedule,
    WeeklySchedule,
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
        DaySchedule {
            is_working: true,
            start_time: Some(t.into()),
            end_time: Some(t.into()),
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
            end_time: Some("17:00:00".into()),
        };
        let days = [off(), bad.clone(), off(), off(), off(), off(), off()];
        let err = schedule_missing_times(&ws(&days)).unwrap_err();
        assert_eq!(err, "tuesday");
    }

    #[test]
    fn schedule_on_without_end_fails() {
        let bad = DaySchedule {
            is_working: true,
            start_time: Some("09:00:00".into()),
            end_time: None,
        };
        let days = [off(), off(), off(), off(), bad.clone(), off(), off()];
        let err = schedule_missing_times(&ws(&days)).unwrap_err();
        assert_eq!(err, "friday");
    }
}
