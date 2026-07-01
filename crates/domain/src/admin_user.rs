//! Admin user creation port — wraps `dbo.SP_USER_INSERT_FULL`.
//!
//! SP_USER_INSERT_FULL is the **rich** admin-side user-creation flow
//! (the legacy ASP.NET admin form uses it). It accepts every
//! detail the admin form collects in one round trip:
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

/// Schedule for a single weekday (`is_working` + optional times).
///
/// The SP enforces: when `is_working = true`, both `start_time`
/// and `end_time` must be non-NULL. When `is_working = false`,
/// both fields are ignored (the SP still inserts NULL). Mirrors
/// `dbo.user_work_day_template` column-by-column.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DaySchedule {
    /// Whether this weekday is a working day.
    pub is_working: bool,
    /// `HH:MM:SS` string (DB type `time(0)`). `None` when
    /// `is_working = false`.
    pub start_time: Option<String>,
    /// `HH:MM:SS` string (DB type `time(0)`). `None` when
    /// `is_working = false`.
    pub end_time: Option<String>,
}

/// Weekly working schedule template — one row per weekday.
///
/// Field order matches the SP parameter order (`monday_*` ...
/// `sunday_*`) so the infra layer can pass them through
/// positionally without renaming.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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
/// ponytail: `keyword` and `user_status` are the two filter axes the
/// SP exposes. The ceiling is adding `role_code` / `position_guid` /
/// date-range filters — at that point promote to a builder pattern so
/// the SP signature stays flat while the Rust side stays readable.
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
///   `total_count`       (bigint — same value on every row of a page)
///   `page`              (int     — echo of `@p_page`)
///   `page_size`         (int     — echo of `@p_page_size`)
///   `user_guid`         (varchar 36 — `[user].user_guid`)
///   `full_name`         (varchar — COALESCEd to "")
///   `phone`             (varchar — `[user].user_tel`, COALESCEd to "")
///   `user_status`       (int — 0..3, see note below)
///   `user_status_name`  (varchar — "Inactive"/"Active"/"Suspended"/"Deleted")
///   `role_name`         (varchar CSV — COALESCEd "", already joined at SP level)
///   `position_name`     (varchar — COALESCEd "", the *current* position)
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
    /// Active role names, comma-joined (e.g. `"Admin, Finance Manager"`).
    /// The SP applies `STRING_AGG` with `DISTINCT` and an active-only
    /// filter — we keep it as a single `String` to match the SP shape
    /// 1:1 (the admin UI splits on `,` for badge rendering).
    pub role_name: String,
    /// Current position name (`master_position.master_position_name`),
    /// COALESCEd to "" when the user has no current position row.
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
