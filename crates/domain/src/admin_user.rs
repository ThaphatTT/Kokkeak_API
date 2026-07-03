//! Admin user ports — wrap the rich admin-side `user` stored
//! procedures.
//!
//! ## `SP_USER_INSERT_FULL` (admin user creation)
//!
//! SP_USER_INSERT_FULL accepts every detail the admin form collects
//! in one round trip:
//!
//! - basic profile (first / last name, id_card, tel, email, gender)
//! - address (country / province / district / sub_district / village / post)
//! - flags (`is_foreign`, `is_customer_company`, `is_customer`,
//!   `is_admin`, `is_employee`, `is_freelance`, status)
//! - login (`username` + already-hashed `password_hash`)
//! - profile image path
//! - company binding (`company_guid` + name / tel / type / status)
//! - department + department_team scope
//! - position + start date
//! - salary (decimal + currency, defaults to THB)
//! - weekly working schedule (mon–sun, each day = `is_working`
//!   + `start_time` + `end_time`)
//! - bank account (name / code / account_no / account_name / book image)
//! - 4 attachment paths (id_card_front, id_card_back,
//!   proof_of_address, source_of_funds_statement)
//!
//! ## `SP_USER_UPDATE_FULL` (admin user update)
//!
//! SP_USER_UPDATE_FULL is the write-side counterpart to the
//! detail-read SP — it accepts the same per-field admin-form
//! payload and updates the matching `[user]` row + the linked
//! `[user_username]` row. Differences from INSERT:
//!
//! - `@p_user_guid` is **required** (no NEWID fallback).
//! - No password field — password reset lives on a separate
//!   flow (out of scope here).
//! - The SP emits `USER_NOT_FOUND` when the GUID doesn't
//!   resolve to a non-deleted row (insert doesn't have this
//!   case).
//!
//! ## `SP_USER_DETAIL_FULL_GET` (admin user detail lookup)
//!
//! SP_USER_DETAIL_FULL_GET is the read-side counterpart: one row per
//! `user_guid` with every related detail assembled via `OUTER APPLY`
//! blocks (profile image, company, roles, department/team, current
//! position, current salary, working schedule, default bank
//! account, four attachment paths). The Rust shape is a single
//! [`AdminUserDetail`] with optional sub-structs — each sub-block is
//! `None` when the user has no matching row (e.g. no company, no
//! current position, no salary yet).
//!
//! The actor (admin creating the user) is identified by
//! `user_username_guid` — the SP resolves it to `user_guid` via
//! `dbo.FN_SECURITY_RESOLVE_USER_GUID_BY_USERNAME_GUID` and
//! re-checks the `ADMIN` role server-side as defense-in-depth (the
//! handler already gates on `admin_flag`, so this is belt + braces).
//!
//! ## Role assignment logic
//!
//! Mirrors the SP's role-pick rules verbatim so callers can reason
//! about it without re-reading the SP body:
//!
//! - `is_admin = 1` → assign role `ADMIN` (wins over employee)
//! - `is_admin = 0`, `is_employee = 1` → assign role `EMPLOYEE`
//! - both `= 0` → no role assigned
//!
//! ## Attachment type codes (mirrors the SP's documentation)
//!
//! - `1` = ID Card Front
//! - `2` = ID Card Back
//! - `3` = Proof of Address
//! - `4` = Source of Funds Statement
//!
//! ## Failure model
//!
//! The SP returns one row with `success` (bit) + `code` (varchar)
//! + `message` (varchar) + the optional GUIDs. On failure the
//!   `code` is one of a stable set of snake_case strings (see the
//!   `code → http status` mapping in
//!   `crates/api/src/handlers/admin.rs::sp_insert_full_status`).
//!
//! The Rust side surfaces the SP failure as
//! [`AdminInsertUserError`] — the `code` is forwarded to the wire
//! as the `error.code` (via the `ErrorCode` catalog) so mobile
//! clients can pattern-match on it.
//!
//! Update failures use the parallel [`AdminUpdateUserError`]
//! shape (same `code` + `message` fields) and are mapped in
//! `handlers::admin::sp_update_full_status`. The two error
//! structs are distinct so future drift between the insert
//! and update SPs (e.g. update adds a new code) doesn't
//! pollute the insert contract.

use rust_decimal::Decimal;

/// Successful output of `SP_USER_INSERT_FULL`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminInsertUserResult {
    /// Newly-created `[user].user_guid` (36-char UUID string).
    pub user_guid: String,
    /// Newly-created `[user_username].user_username_guid` (36-char UUID string).
    pub user_username_guid: String,
    /// The username that was just registered (echoed by the SP).
    pub username: String,
    /// `user_role_guid` that the SP assigned (ADMIN / EMPLOYEE /
    /// `None` when neither flag was set).
    pub assigned_role_guid: Option<String>,
}

/// Structured failure returned by `SP_USER_INSERT_FULL`.
///
/// `code` is one of the SP's stable snake_case strings (see the
/// `code → http status` mapping table in
/// `crates/api/src/handlers/admin.rs`). The `message` is the
/// human-readable English string the SP emitted — the API
/// surfaces it verbatim in the localized envelope (admin
/// operators don't get a translated UI; the SP messages are
/// English by design).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("SP_USER_INSERT_FULL failed: {code} — {message}")]
pub struct AdminInsertUserError {
    /// Stable SP error code (e.g. `USERNAME_EXISTS`, `PERMISSION_DENIED`).
    pub code: String,
    /// Human-readable English description from the SP.
    pub message: String,
}

impl AdminInsertUserError {
    /// Construct from the SP's `code` + `message` columns.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Successful output of `SP_USER_UPDATE_FULL`.
///
/// `SP_USER_UPDATE_FULL` echoes only the resolved `user_guid`
/// (the caller already supplied it). The admin UI already knows
/// the username + the full record (it called GET first to
/// pre-fill the form) so no extra fields are echoed on the
/// success path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminUpdateUserResult {
    /// The `[user].user_guid` that was updated (echoed by the SP).
    pub user_guid: String,
}

/// Structured failure returned by `SP_USER_UPDATE_FULL`.
///
/// `code` is one of the SP's stable snake_case strings. The handler
/// maps `code` → HTTP status + `error.code` via
/// `sp_update_full_status` (mirrors the insert mapping table but
/// drops `password_hash_required` and `user_guid_exists`, adds
/// `user_not_found`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("SP_USER_UPDATE_FULL failed: {code} — {message}")]
pub struct AdminUpdateUserError {
    /// Stable SP error code (e.g. `USER_NOT_FOUND`, `USERNAME_EXISTS`).
    pub code: String,
    /// Human-readable English description from the SP.
    pub message: String,
}

impl AdminUpdateUserError {
    /// Construct from the SP's `code` + `message` columns.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Schedule for a single weekday (`is_working` + optional times).
///
/// The SP enforces: when `is_working = true`, both `start_time`
/// and `end_time` must be non-NULL. When `is_working = false`,
/// both fields are ignored (the SP still inserts NULL). Mirrors
/// `dbo.user_work_day_template` column-by-column.
///
/// `start_time` / `end_time` are typed as [`chrono::NaiveTime`] so
/// the domain speaks the DB's `time(0)` type natively (no string
/// round-trip on the read path; the wire JSON still serialises as
/// `"HH:MM:SS"` via chrono's default serde implementation). The
/// tiberius `chrono` feature binds `time(0)` columns directly to
/// `NaiveTime` — the infra mapper reads them via
/// `row.get::<NaiveTime, _>(...)` and the write-side binds them
/// as `"HH:MM:SS"` strings (the SP accepts either form).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DaySchedule {
    /// Whether this weekday is a working day.
    pub is_working: bool,
    /// `time(0)` value (DB column). `None` when `is_working = false`.
    pub start_time: Option<chrono::NaiveTime>,
    /// `time(0)` value (DB column). `None` when `is_working = false`.
    pub end_time: Option<chrono::NaiveTime>,
}

/// Weekly working schedule template — one row per weekday.
///
/// Field order matches the SP parameter order (`monday_*` ...
/// `sunday_*`) so the infra layer can pass them through
/// positionally without renaming.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WeeklySchedule {
    /// Monday schedule.
    pub monday: DaySchedule,
    /// Tuesday schedule.
    pub tuesday: DaySchedule,
    /// Wednesday schedule.
    pub wednesday: DaySchedule,
    /// Thursday schedule.
    pub thursday: DaySchedule,
    /// Friday schedule.
    pub friday: DaySchedule,
    /// Saturday schedule.
    pub saturday: DaySchedule,
    /// Sunday schedule.
    pub sunday: DaySchedule,
}

/// Full input to `SP_USER_INSERT_FULL`.
///
/// Every field maps 1:1 to a SP parameter. The Rust type is
/// intentionally flat (no nested struct for address / bank /
/// position) so the infra layer can iterate `params` once when
/// building the EXEC — the SP signature is already flat.
///
/// ponytail: the field-level `///` docs are intentionally terse
/// (one line each) because every field is a 1:1 mirror of a SP
/// parameter — the module-level doc above carries the
/// design rationale + role-pick rules. Ceiling: when the SP
/// evolves (e.g. splits into multiple sub-procedures), break
/// this struct into `AdminInsertUserBasic` /
/// `AdminInsertUserAddress` / `AdminInsertUserWorkSchedule`
/// sub-structs and let serde flatten them — at that point the
/// per-field docs become the primary reference.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(missing_docs)]
pub struct AdminInsertUserRequest {
    /// The admin's `user_username_guid` (NOT `user_guid`).
    /// Looked up via `find_username_guid_by_user_guid` before the
    /// SP call so the handler never has to expose the column.
    pub actor_user_username_guid: String,

    // ---- User Basic ----
    /// Optional. The SP generates a NEWID when NULL/empty.
    pub user_guid: Option<String>,
    /// `user_first_name` — required by the SP.
    pub first_name: String,
    /// `user_last_name` — required by the SP.
    pub last_name: String,
    /// `user_id_card` — optional.
    pub id_card: Option<String>,
    /// `user_tel` — optional.
    pub tel: Option<String>,
    /// `user_email` — required by the SP.
    pub email: String,
    /// `user_gender` — optional free-form string.
    pub gender: Option<String>,

    // ---- Address ----
    /// `user_country_guid` — required when `is_foreign = 1`.
    pub country_guid: Option<String>,
    /// `user_province`.
    pub province: Option<String>,
    /// `user_district`.
    pub district: Option<String>,
    /// `user_sub_district`.
    pub sub_district: Option<String>,
    /// `user_village`.
    pub village: Option<String>,
    /// `user_post` (postal code).
    pub post: Option<String>,

    /// `user_description` — free-form bio.
    pub description: Option<String>,

    // ---- Flags ----
    /// `user_is_foreign` — switches on country / postal validation.
    pub is_foreign: bool,
    /// `user_is_customer_company` — switches on company validation.
    pub is_customer_company: bool,
    /// `user_is_customer` — tag, not used by the SP for validation.
    pub is_customer: bool,
    /// `user_is_admin` — picks the ADMIN role (wins over EMPLOYEE).
    pub is_admin: bool,
    /// `user_is_employee` — picks the EMPLOYEE role (only when
    /// `is_admin = 0`).
    pub is_employee: bool,
    /// `user_is_freelance` — tag.
    pub is_freelance: bool,
    /// `user_status`: 1 = active, 0 = inactive. Default 1.
    pub status: i32,

    // ---- Login ----
    /// `user_username_username` — required by the SP.
    pub username: String,
    /// **Already-hashed** argon2id PHC string. The service hashes
    /// the request DTO's `password` BEFORE building this struct —
    /// the SP never sees plaintext.
    pub password_hash: String,

    // ---- Profile image ----
    /// `user_img_profile_img_path` — primary profile image path.
    pub profile_img_path: Option<String>,

    // ---- Company ----
    /// `user_company_company_guid` — required when
    /// `is_customer_company = 1`.
    pub company_guid: Option<String>,
    /// `user_company_name`.
    pub company_name: Option<String>,
    /// `user_company_tel`.
    pub company_tel: Option<String>,
    /// `user_company_type` — free-form int (legacy code uses
    /// `int`, not enum). Default 1.
    pub company_type: Option<i32>,
    /// `user_company_status` — default 1 (active).
    pub company_status: i32,

    // ---- Department / team scope ----
    /// `user_department_guid`.
    pub department_guid: Option<String>,
    /// `user_department_team_guid` — must belong to `department_guid`.
    pub department_team_guid: Option<String>,

    // ---- Position ----
    /// `master_position_guid`.
    pub position_guid: Option<String>,
    /// `user_position_start_at` — defaults to `SYSUTCDATETIME()`.
    pub position_start_at: Option<chrono::DateTime<chrono::Utc>>,

    // ---- Salary ----
    /// `user_salary_amount` — `decimal(18,2)` (use
    /// `rust_decimal::Decimal`, never `f64`).
    pub salary_amount: Option<Decimal>,
    /// `user_salary_currency` — defaults to `"THB"` server-side;
    /// pass `None` to accept.
    pub salary_currency: Option<String>,

    // ---- Working schedule ----
    /// Weekly schedule (monday..sunday).
    pub schedule: WeeklySchedule,

    // ---- Bank ----
    /// `user_bank_account_bank_name`.
    pub bank_name: Option<String>,
    /// `user_bank_account_bank_code`.
    pub bank_code: Option<String>,
    /// `user_bank_account_no`.
    pub bank_account_no: Option<String>,
    /// `user_bank_account_name` (account-holder name).
    pub bank_account_name: Option<String>,
    /// `user_bank_account_book_img_path` — book-cover image.
    pub bank_book_img_path: Option<String>,

    // ---- Attachments (types 1..4) ----
    /// Type 1 — ID Card Front.
    pub id_card_front_path: Option<String>,
    /// Type 2 — ID Card Back.
    pub id_card_back_path: Option<String>,
    /// Type 3 — Proof of Address.
    pub proof_of_address_path: Option<String>,
    /// Type 4 — Source of Funds Statement.
    pub source_of_funds_statement_path: Option<String>,
}

/// Full input to `SP_USER_UPDATE_FULL`.
///
/// Mirrors [`AdminInsertUserRequest`] 1:1 with two differences:
///
/// 1. `user_guid` is **required** — the SP needs to know which
///    row to update. The Rust URL contract puts it in the path
///    (`PUT /api/v1/admin/users/:guid/full`) so the field is a
///    `String`, not `Option<String>`.
/// 2. **No password field** — updating the password is a separate
///    concern (`SP_USER_PASSWORD_RESET` lives outside this
///    endpoint). The actor admin can issue a reset from the user
///    detail screen.
///
/// Field order matches the SP parameter order so the infra
/// layer can pass them through positionally without renaming.
///
/// ponytail: the duplicate shape between insert / update is
/// intentional. Two parallel structs (vs. one shared `User` +
/// flags) keeps each SP's parameter contract obvious — when the
/// insert and update SPs drift in the future (e.g. update gains a
/// `last_login_at` write), only the affected struct needs to
/// change.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(missing_docs)]
pub struct AdminUpdateUserRequest {
    /// The admin's `user_username_guid` (NOT `user_guid`).
    /// Looked up via `find_username_guid_by_user_guid` before the
    /// SP call so the handler never has to expose the column.
    pub actor_user_username_guid: String,

    // ---- Target user ----
    /// The `[user].user_guid` to update — required by the SP.
    pub user_guid: String,

    // ---- User Basic ----
    /// `user_first_name` — required by the SP.
    pub first_name: String,
    /// `user_last_name` — required by the SP.
    pub last_name: String,
    /// `user_id_card` — optional.
    pub id_card: Option<String>,
    /// `user_tel` — optional.
    pub tel: Option<String>,
    /// `user_email` — required by the SP.
    pub email: String,
    /// `user_gender` — optional free-form string.
    pub gender: Option<String>,

    // ---- Address ----
    /// `user_country_guid` — required when `is_foreign = 1`.
    pub country_guid: Option<String>,
    /// `user_province`.
    pub province: Option<String>,
    /// `user_district`.
    pub district: Option<String>,
    /// `user_sub_district`.
    pub sub_district: Option<String>,
    /// `user_village`.
    pub village: Option<String>,
    /// `user_post` (postal code).
    pub post: Option<String>,

    /// `user_description` — free-form bio.
    pub description: Option<String>,

    // ---- Flags ----
    /// `user_is_foreign` — switches on country / postal validation.
    pub is_foreign: bool,
    /// `user_is_customer_company` — switches on company validation.
    pub is_customer_company: bool,
    /// `user_is_customer` — tag, not used by the SP for validation.
    pub is_customer: bool,
    /// `user_is_admin` — picks the ADMIN role (wins over EMPLOYEE).
    pub is_admin: bool,
    /// `user_is_employee` — picks the EMPLOYEE role (only when
    /// `is_admin = 0`).
    pub is_employee: bool,
    /// `user_is_freelance` — tag.
    pub is_freelance: bool,
    /// `user_status`: 1 = active, 0 = inactive. Default 1.
    pub status: i32,

    // ---- Login ----
    /// `user_username_username` — required by the SP.
    pub username: String,

    // ---- Profile image ----
    /// `user_img_profile_img_path` — primary profile image path.
    pub profile_img_path: Option<String>,

    // ---- Company ----
    /// `user_company_company_guid` — required when
    /// `is_customer_company = 1`.
    pub company_guid: Option<String>,
    /// `user_company_name`.
    pub company_name: Option<String>,
    /// `user_company_tel`.
    pub company_tel: Option<String>,
    /// `user_company_type` — free-form int (legacy code uses
    /// `int`, not enum). Default 1.
    pub company_type: Option<i32>,
    /// `user_company_status` — default 1 (active).
    pub company_status: i32,

    // ---- Department / team scope ----
    /// `user_department_guid`.
    pub department_guid: Option<String>,
    /// `user_department_team_guid` — must belong to `department_guid`.
    pub department_team_guid: Option<String>,

    // ---- Position ----
    /// `master_position_guid`.
    pub position_guid: Option<String>,
    /// `user_position_start_at` — defaults to `SYSUTCDATETIME()`.
    pub position_start_at: Option<chrono::DateTime<chrono::Utc>>,

    // ---- Salary ----
    /// `user_salary_amount` — `decimal(18,2)` (use
    /// `rust_decimal::Decimal`, never `f64`).
    pub salary_amount: Option<Decimal>,
    /// `user_salary_currency` — defaults to `"THB"` server-side;
    /// pass `None` to accept.
    pub salary_currency: Option<String>,

    // ---- Working schedule ----
    /// Weekly schedule (monday..sunday).
    pub schedule: WeeklySchedule,

    // ---- Bank ----
    /// `user_bank_account_bank_name`.
    pub bank_name: Option<String>,
    /// `user_bank_account_bank_code`.
    pub bank_code: Option<String>,
    /// `user_bank_account_no`.
    pub bank_account_no: Option<String>,
    /// `user_bank_account_name` (account-holder name).
    pub bank_account_name: Option<String>,
    /// `user_bank_account_book_img_path` — book-cover image.
    pub bank_book_img_path: Option<String>,

    // ---- Attachments (types 1..4) ----
    /// Type 1 — ID Card Front.
    pub id_card_front_path: Option<String>,
    /// Type 2 — ID Card Back.
    pub id_card_back_path: Option<String>,
    /// Type 3 — Proof of Address.
    pub proof_of_address_path: Option<String>,
    /// Type 4 — Source of Funds Statement.
    pub source_of_funds_statement_path: Option<String>,
}

// ============================================================================
// Admin user listing — wraps `dbo.SP_USER_LIST_PAGING`.
// ============================================================================
//
// Distinct from [`UserListRow`](crate::user::UserListRow) (which mirrors
// `SP_PERMISSION_USER_LIST` for the permission-page view). The admin
// user-list screen needs a **wider** row — the legacy ASP.NET form shows
// status label / phone / role name(s) / position name per user — and
// uses **page-based pagination** rather than keyset.
//
// ponytail: two distinct row types share a similar prefix because they
// back two distinct UIs (permission page vs. user list page). The
// ceiling is when the admin screen wants to merge both views — at that
// point widen the SP to return both shapes and pick a single Rust
// struct; for now a one-to-one mirror of the SP keeps the mapper
// trivial.

/// Filter / paging parameters for `SP_USER_LIST_PAGING`.
///
/// Mirrors the SP's parameter list 1:1 so the infra layer can pass
/// them through positionally without renaming.
///
/// The SP supports ten parameters: four paging/keyword/status and
/// six scope filters (user-type booleans + department/team/position
/// GUIDs). The application layer is responsible for normalising
/// `keyword` (trim) and clamping `page` / `page_size`; the SP itself
/// only echoes the paging knobs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(missing_docs)]
pub struct AdminUserListPagingInput {
    /// `@p_keyword` — free-form search across first/last name, phone,
    /// email, and `"<first> <last>"`. Empty string (or all-whitespace)
    /// means "no keyword filter".
    pub keyword: String,
    /// `@p_user_status` — `None` means "all statuses" (the SP itself
    /// never receives `NULL`, it gets a sentinel — see infra layer).
    pub user_status: Option<i32>,
    /// `@p_user_is_customer` — `Some(true)` returns customer-flagged
    /// users only; `Some(false)` returns non-customers; `None` skips
    /// the filter (the SP receives `NULL`).
    pub user_is_customer: Option<bool>,
    /// `@p_user_is_employee` — same semantics as
    /// [`user_is_customer`](Self::user_is_customer).
    pub user_is_employee: Option<bool>,
    /// `@p_user_is_freelance` — same semantics as
    /// [`user_is_customer`](Self::user_is_customer). Freelance
    /// technicians are a separate cohort from employees.
    pub user_is_freelance: Option<bool>,
    /// `@p_department_guid` — restrict to users whose
    /// `user_user_role.user_user_role_department_guid` matches and
    /// whose role assignment is still active. `None` = no filter.
    pub department_guid: Option<String>,
    /// `@p_department_team_guid` — same active-role semantics as
    /// [`department_guid`](Self::department_guid), scoped to team.
    pub department_team_guid: Option<String>,
    /// `@p_position_guid` — restrict to users whose
    /// `user_position` is current (`is_current = 1`) and
    /// `master_position_guid` matches. `None` = no filter.
    pub position_guid: Option<String>,
    /// `@p_page` — 1-based page number. The SP defaults to 1 when
    /// the input is `< 1`.
    pub page: u32,
    /// `@p_page_size` — rows per page. The SP defaults to 20 / caps
    /// at 100; the application layer mirrors the same bounds.
    pub page_size: u32,
}

/// One row of `SP_USER_LIST_PAGING` — flat per-user summary for the
/// admin user-list screen.
///
/// Column NAMES match the SP's SELECT aliases verbatim:
///   `total_count`             (bigint  — same value on every row of a page)
///   `page`                    (int     — echo of `@p_page`)
///   `page_size`               (int     — echo of `@p_page_size`)
///   `user_guid`               (varchar 36 — `[user].user_guid`)
///   `full_name`               (varchar — COALESCEd to "")
///   `phone`                   (varchar — `[user].user_tel`, COALESCEd to "")
///   `user_status`             (int — 0..3, see note below)
///   `user_status_name`        (varchar — "Inactive"/"Active"/"Suspended"/"Deleted")
///   `user_is_customer`        (bit — `[user].user_is_customer`)
///   `user_is_employee`        (bit — `[user].user_is_employee`)
///   `user_is_freelance`       (bit — `[user].user_is_freelance`)
///   `role_name`               (varchar CSV — COALESCEd "", already joined at SP level)
///   `department_guid`         (varchar 36 — most-recent active role's department)
///   `department_name`         (varchar — joined from `user_department`)
///   `department_team_guid`    (varchar 36 — most-recent active role's team)
///   `department_team_name`    (varchar — joined from `user_department_team`)
///   `position_guid`           (varchar 36 — current position's master)
///   `position_name`           (varchar — COALESCEd "", the *current* position)
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UserListPagingRow {
    /// Total rows matching the filter (not the page size). SP returns
    /// the same value on every row of a page — application layer
    /// collapses to a single number.
    pub total_count: i64,
    /// Echoed page index (1-based). Useful when the caller paginates
    /// client-side and wants to confirm the SP saw the same `page`
    /// it sent.
    pub page: i32,
    /// Echoed page size. Same use as `page` — surfaces any
    /// server-side clamping the SP applied.
    pub page_size: i32,
    /// `[user].user_guid` (36-char UUID).
    pub user_guid: String,
    /// `first_name + ' ' + last_name` from `[user]`, COALESCEd to "".
    pub full_name: String,
    /// `[user].user_tel`, COALESCEd to "".
    pub phone: String,
    /// `[user].user_status` raw `int` (0..3). The caller should
    /// prefer [`user_status_name`](Self::user_status_name) for
    /// display, but the raw int is kept for any future "filter
    /// by status" UX without forcing a re-fetch.
    ///
    /// **Note on semantics**: the SP aliases `0` as `"Inactive"`
    /// (legacy label) — the NEW_DB Rust enum ([`crate::user::UserStatus`])
    /// uses `"pending"` for `0`. The legacy SP's labels are what the
    /// admin UI shows today, so we surface the SP string verbatim
    /// and let the Rust enum diverge until the admin screen is
    /// migrated to the new vocabulary.
    pub user_status: i32,
    /// Human-readable status label as computed by the SP:
    /// `Inactive` / `Active` / `Suspended` / `Deleted` / `Unknown`.
    pub user_status_name: String,
    /// `[user].user_is_customer` — `true` for the customer cohort,
    /// `false` for staff / technicians. Sourced directly from the
    /// `[user]` row (the SP `CAST(ISNULL(..., 0) AS bit)`s it).
    pub user_is_customer: bool,
    /// `[user].user_is_employee` — `true` for the staff cohort
    /// (admins, finance, ops, etc.). Independent from
    /// [`user_is_customer`](Self::user_is_customer); a user can
    /// theoretically carry both flags depending on legacy data.
    pub user_is_employee: bool,
    /// `[user].user_is_freelance` — `true` for freelance
    /// technicians (a third cohort distinct from employees and
    /// customers).
    pub user_is_freelance: bool,
    /// Active role names, comma-joined (e.g. `"Admin, Finance Manager"`).
    /// The SP applies `STRING_AGG` with `DISTINCT` and an active-only
    /// filter — we keep it as a single `String` to match the SP shape
    /// 1:1 (the admin UI splits on `,` for badge rendering).
    pub role_name: String,
    /// GUID of the most-recently-assigned **active** department
    /// (from `user_user_role.user_user_role_department_guid`,
    /// ordered by `assigned_at DESC, created_at DESC`).
    /// `""` when the user has no active role row.
    pub department_guid: String,
    /// Human-readable name of the department, joined from
    /// `[user_department]`. `""` when no match.
    pub department_name: String,
    /// GUID of the most-recently-assigned **active** team
    /// (from `user_user_role.user_user_role_department_team_guid`).
    /// `""` when the user has no active role row.
    pub department_team_guid: String,
    /// Human-readable name of the team, joined from
    /// `[user_department_team]`. `""` when no match.
    pub department_team_name: String,
    /// GUID of the user's **current** position (the
    /// `user_position` row where `is_current = 1` and
    /// `end_at` is in the future / NULL). `""` when the user
    /// has no current position.
    pub position_guid: String,
    /// Human-readable position name (`master_position.master_position_name`).
    /// `""` when the user has no current position row.
    pub position_name: String,
}

/// One page of admin user-listing results.
///
/// `total_count` lives on the row level (the SP returns it on every
/// row); the application layer hoists it to the page so the wire
/// envelope carries it once.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AdminUserListPagingPage {
    /// Rows on this page (each row also carries `total_count`,
    /// `page`, `page_size` — the page-level fields below are the
    /// authoritative ones for the envelope).
    pub items: Vec<UserListPagingRow>,
    /// Total matching rows across all pages (1+ when items is non-empty;
    /// `0` when no rows match the filter).
    pub total_count: i64,
    /// Page index returned (1-based).
    pub page: i32,
    /// Page size returned.
    pub page_size: i32,
}

// ============================================================================
// M22: GET /api/v1/admin/users/:guid/detail  (SP_USER_DETAIL_FULL_GET)
// ============================================================================
//
// Wire shape for the admin user-detail screen. Mirrors
// `dbo.SP_USER_DETAIL_FULL_GET` column-by-column. Every sub-block
// (profile image, company, role, department, current position,
// current salary, working schedule, default bank account, four
// attachment paths) is wrapped in `Option<_>` so a user with no
// company / no current position / no bank account still serialises
// cleanly (the SP `OUTER APPLY` blocks return no row → the mapper
// emits `None`).
//
// Field names use the SAME snake_case aliases the SP emits so the
// infra mapper can read by column name without renaming.
//
// ponytail: all sub-structs are flat (no nested address/bank/etc.)
// to keep the wire payload 1:1 with the SP's SELECT list. When the
// SP grows new sub-blocks (e.g. emergency contact), add another
// `Option<NewSubBlock>` here + a row mapper in the infra layer.

/// Profile image row from `[user_img_profile]` (most recent active).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[allow(missing_docs)]
pub struct AdminUserDetailProfileImage {
    /// `[user_img_profile].user_img_profile_guid` (36-char UUID).
    pub user_img_profile_guid: String,
    /// Storage path under `users/{guid}/profile/{uuid}.webp`.
    /// `""` when no image is set yet.
    pub profile_img_path: String,
    /// T-23: client-facing URL for the image above (e.g.
    /// `https://api.sdplao.com/files/users/{guid}/profile/{uuid}.webp`).
    /// `None` when no base URL is configured or the path is empty
    /// — the API edge composes this from `Settings::server::public_base_url`
    /// plus the path. The infra row mapper sets it to `None`; the
    /// handler fills it before serialising.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_img_url: Option<String>,
}

/// Company binding row from `[user_company]` (most recent active) +
/// joined `[company]` fields.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[allow(missing_docs)]
pub struct AdminUserDetailCompany {
    /// `[user_company].user_company_guid` (36-char UUID).
    pub user_company_guid: String,
    /// `[company].company_guid` (36-char UUID) — `""` when the
    /// user-company row exists without a master-company link.
    pub company_guid: String,
    /// Master `[company].company_name`. `""` when not set.
    pub company_name: String,
    /// Master `[company].company_tel`. `""` when not set.
    pub company_tel: String,
    /// `[user_company].user_company_name` — per-user display name.
    pub user_company_name: String,
    /// `[user_company].user_company_tel` — per-user contact tel.
    pub user_company_tel: String,
    /// `[user_company].user_company_type` (int).
    pub user_company_type: i32,
    /// `[user_company].user_company_status` (int 0/1).
    pub user_company_status: i32,
}

/// Aggregated role data from `[user_user_role]` + `[user_role]`.
///
/// `role_codes` is a comma-separated string (`customer,admin,…`).
/// `role_names` is a comma-separated human-readable list (admin UI
/// splits on `,` for badge rendering — same convention as
/// [`UserListPagingRow::role_name`]).
/// `user_is_admin` is the `OR` of every role with `role_code = 'ADMIN'`
/// (the SP computes this so the Rust side doesn't have to walk the
/// CSV).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[allow(missing_docs)]
pub struct AdminUserDetailRoles {
    /// Comma-separated role codes (e.g. `"customer,admin"`).
    pub role_codes: String,
    /// Comma-separated role names (e.g. `"Customer, Admin"`).
    pub role_names: String,
    /// `true` if any active role carries the `ADMIN` code.
    pub user_is_admin: bool,
}

/// Department / department-team scope from the most recent active
/// `[user_user_role]` row.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[allow(missing_docs)]
pub struct AdminUserDetailScope {
    /// `[user_department].user_department_guid` (36-char UUID).
    pub department_guid: String,
    /// `[user_department].user_department_code` — admin UI lookup key.
    pub department_code: String,
    /// `[user_department].user_department_name` — display name.
    pub department_name: String,
    /// `[user_department_team].user_department_team_guid` (36-char UUID).
    pub department_team_guid: String,
    /// `[user_department_team].user_department_team_code`.
    pub department_team_code: String,
    /// `[user_department_team].user_department_team_name`.
    pub department_team_name: String,
}

/// Current position row from `[user_position]` (most recent active +
/// `is_current = 1`) + joined `[master_position]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[allow(missing_docs)]
pub struct AdminUserDetailPosition {
    /// `[user_position].user_position_guid` (36-char UUID).
    pub user_position_guid: String,
    /// `[master_position].master_position_guid` (36-char UUID).
    pub position_guid: String,
    /// `[master_position].master_position_code`.
    pub position_code: String,
    /// `[master_position].master_position_name` — display name.
    pub position_name: String,
    /// `[master_position].master_position_level` (int 0..N). The SP
    /// emits NULL for positions without an explicit level — the
    /// mapper surfaces that as `0` (the same convention
    /// `MasterPositionAutocompleteRow.level` uses so a missing
    /// level sorts last under `ORDER BY level DESC`).
    pub position_level: i32,
    /// `[user_position].user_position_start_at` (UTC).
    pub position_start_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `[user_position].user_position_end_at` (UTC) — `None` when open-ended.
    pub position_end_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Current salary row from `[user_salary]` (`is_current = 1`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[allow(missing_docs)]
pub struct AdminUserDetailSalary {
    /// `[user_salary].user_salary_guid` (36-char UUID).
    pub user_salary_guid: String,
    /// `[user_salary].user_salary_amount` (decimal).
    pub salary_amount: Decimal,
    /// `[user_salary].user_salary_currency` (e.g. `"THB"`).
    pub salary_currency: String,
    /// `[user_salary].user_salary_type` (int).
    pub salary_type: i32,
    /// `[user_salary].user_salary_effective_from` (UTC).
    pub salary_effective_from: Option<chrono::DateTime<chrono::Utc>>,
    /// `[user_salary].user_salary_effective_to` (UTC) — `None` when open-ended.
    pub salary_effective_to: Option<chrono::DateTime<chrono::Utc>>,
}

/// Default bank account row from `[user_bank_account]` (most recent
/// active + `is_default = 1` first).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[allow(missing_docs)]
pub struct AdminUserDetailBankAccount {
    /// `[user_bank_account].user_bank_account_guid` (36-char UUID).
    pub user_bank_account_guid: String,
    /// `[user_bank_account].user_bank_account_bank_name`.
    pub bank_name: String,
    /// `[user_bank_account].user_bank_account_bank_code`.
    pub bank_code: String,
    /// `[user_bank_account].user_bank_account_branch_name`.
    pub branch_name: String,
    /// `[user_bank_account].user_bank_account_name`.
    pub bank_account_name: String,
    /// `[user_bank_account].user_bank_account_no` — full account number.
    /// Never log (PII). The masked variant is what the admin UI shows.
    pub bank_account_no: String,
    /// `[user_bank_account].user_bank_account_no_masked` — e.g. `"xxx-x-12345"`.
    pub bank_account_no_masked: String,
    /// `[user_bank_account].user_bank_account_type` (int).
    pub bank_account_type: i32,
    /// `[user_bank_account].user_bank_account_is_default` (bool).
    pub bank_account_is_default: bool,
    /// `[user_bank_account].user_bank_account_verified_status` (int).
    pub bank_account_verified_status: i32,
    /// `[user_bank_account].user_bank_account_book_img_path`.
    pub bank_book_img_path: String,
    /// T-23: client-facing URL for the bank-book image above.
    /// `None` until the API edge composes it from `public_base_url`
    /// + the storage path; see `AdminUserDetailProfileImage::profile_img_url`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bank_book_img_url: Option<String>,
}

/// One attachment row from `[user_details_attachment]` — most recent
/// active row of the given type.
///
/// The SP returns four separate columns keyed by
/// `user_details_attachment_type` (`1`=front, `2`=back, `3`=proof,
/// `4`=source-of-funds); the Rust shape reuses this struct for all
/// four slots.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[allow(missing_docs)]
pub struct AdminUserDetailAttachment {
    /// `[user_details_attachment].user_details_attachment_guid` (36-char UUID).
    pub user_details_attachment_guid: String,
    /// Storage path. `""` when no row of this type exists.
    pub attachment_path: String,
    /// T-23: client-facing URL for the attachment above
    /// (id-card front / back / proof-of-address / source-of-funds).
    /// Composed by the handler from `public_base_url + path`.
    /// `None` when no row exists yet (empty path) or the env
    /// knob is unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment_url: Option<String>,
}

/// Username row from `[user_username]` — most-recent active login.
/// Password hash is NEVER returned (AGENTS.md § 12.1).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[allow(missing_docs)]
pub struct AdminUserDetailUsername {
    /// `[user_username].user_username_guid` (36-char UUID).
    pub user_username_guid: String,
    /// Login username (lowercased canonical form).
    pub username: String,
    /// `[user_username].user_username_status` (raw int).
    pub status: i32,
    /// `[user_username].user_username_create_at` (UTC).
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `[user_username].user_username_update_at` (UTC).
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Country row joined from `[master_country]` — used to enrich the
/// `user_country_guid` reference.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[allow(missing_docs)]
pub struct AdminUserDetailCountry {
    /// `[master_country].master_country_guid` (36-char UUID).
    pub country_guid: String,
    /// `[master_country].master_country_code` (e.g. `"LA"`).
    pub country_code: String,
    /// `[master_country].master_country_name` (e.g. `"Lao PDR"`).
    pub country_name: String,
}

/// Full detail row returned by `SP_USER_DETAIL_FULL_GET`.
///
/// Every sub-block is `Option<_>` because each is fetched via
/// `OUTER APPLY` in the SP — the row exists even when the user has
/// no related company / position / salary / bank account. The
/// handler returns a 404 when the SP emits zero rows entirely
/// (i.e. the `user_guid` doesn't resolve or is soft-deleted).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[allow(missing_docs)]
pub struct AdminUserDetail {
    // ---- User Basic ----
    /// `[user].user_guid` (36-char UUID).
    pub user_guid: String,
    pub user_first_name: String,
    pub user_last_name: String,
    /// `first_name + ' ' + last_name` (SP-computed).
    pub full_name: String,
    pub user_id_card: String,
    pub user_tel: String,
    pub user_email: String,
    pub user_gender: String,

    pub user_is_foreign: bool,
    pub user_country_guid: String,

    pub user_province: String,
    pub user_district: String,
    pub user_sub_district: String,
    pub user_village: String,
    pub user_post: String,
    pub user_description: String,

    pub user_is_customer_company: bool,
    pub user_is_customer: bool,
    pub user_is_employee: bool,
    pub user_is_freelance: bool,
    /// `user_is_admin` — `true` if any active role carries the
    /// `ADMIN` code (SP-computed).
    pub user_is_admin: bool,

    pub user_status: i32,
    /// Human-readable status label as computed by the SP:
    /// `Inactive` / `Active` / `Suspended` / `Deleted` / `Unknown`.
    pub user_status_name: String,

    pub user_create_at: Option<chrono::DateTime<chrono::Utc>>,
    pub user_create_by: String,
    pub user_update_at: Option<chrono::DateTime<chrono::Utc>>,
    pub user_update_by: String,

    // ---- Login ----
    pub username: Option<AdminUserDetailUsername>,

    // ---- Profile Image ----
    pub profile_image: Option<AdminUserDetailProfileImage>,

    // ---- Country ----
    pub country: Option<AdminUserDetailCountry>,

    // ---- Company ----
    pub company: Option<AdminUserDetailCompany>,

    // ---- Role ----
    pub roles: Option<AdminUserDetailRoles>,

    // ---- Department / Department Team ----
    pub scope: Option<AdminUserDetailScope>,

    // ---- Position ----
    pub position: Option<AdminUserDetailPosition>,

    // ---- Salary ----
    pub salary: Option<AdminUserDetailSalary>,

    // ---- Working Schedule ----
    pub schedule: Option<WeeklySchedule>,
    /// `[user_work_day_template].user_work_day_template_guid`
    /// (36-char UUID). `""` when no schedule row exists.
    pub user_work_day_template_guid: String,

    // ---- Bank Account ----
    pub bank_account: Option<AdminUserDetailBankAccount>,

    // ---- Attachments (4 slots, keyed by `user_details_attachment_type`) ----
    pub id_card_front: Option<AdminUserDetailAttachment>,
    pub id_card_back: Option<AdminUserDetailAttachment>,
    pub proof_of_address: Option<AdminUserDetailAttachment>,
    pub source_of_funds_statement: Option<AdminUserDetailAttachment>,
}
