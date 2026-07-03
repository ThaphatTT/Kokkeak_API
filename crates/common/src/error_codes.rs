//! Central catalog of machine-readable error codes (T-17).
//!
//! Every error response carries an `error.code` string in the
//! envelope (see `ApiResponse::error` in [`crate::response`]). The
//! values are stable: once published, a code is **never renamed**
//! — add a new code instead. Mobile / BFF clients should pattern-
//! match on these strings instead of parsing the human message.
//!
//! The full catalog lives in [`ErrorCode`]. Use the constants
//! instead of hand-typing string literals at handler call sites
//! so that typos surface at compile time.
//!
//! ```ignore
//! use kokkak_common::error_codes::ErrorCode;
//! return Err(forbidden(ErrorCode::USERNAME_TAKEN, "..."));
//! ```

/// All stable machine-readable error codes returned by the Kokkeak API.
///
/// Group structure mirrors HTTP status code ranges so the catalog
/// is easy to scan during a postmortem.
pub struct ErrorCode;

impl ErrorCode {
    // ---- 400 Bad Request ----

    /// 400 — request is malformed (invalid JSON, missing required field).
    pub const BAD_REQUEST: &'static str = "bad_request";

    /// 400 — `Idempotency-Key` header is missing or whitespace on a
    /// protected endpoint (`/orders`, `/payments`, `/auth/register`).
    pub const IDEMPOTENCY_KEY_REQUIRED: &'static str = "idempotency_key_required";

    // ---- 401 Unauthorized ----

    /// 401 — credentials missing, wrong, or otherwise invalid.
    pub const UNAUTHORIZED: &'static str = "unauthorized";

    /// 401 — bearer token signature / format invalid.
    pub const INVALID_TOKEN: &'static str = "invalid_token";

    /// 401 — bearer token expired (`exp` claim in the past).
    pub const TOKEN_EXPIRED: &'static str = "token_expired";

    /// 401 — refresh token rejected (revoked, malformed, or expired).
    pub const REFRESH_INVALID: &'static str = "refresh_invalid";

    // ---- 403 Forbidden ----

    /// 403 — authenticated but the role is not allowed on this endpoint.
    /// Generic; prefer a more specific code below when one applies.
    pub const FORBIDDEN: &'static str = "forbidden";

    /// 403 — admin role required (admin-only endpoints).
    pub const ADMIN_REQUIRED: &'static str = "admin_required";

    /// 403 — caller is not a participant of the target chat room.
    pub const NOT_A_PARTICIPANT: &'static str = "not_a_participant";

    // ---- 404 Not Found ----

    /// 404 — resource not found.
    pub const NOT_FOUND: &'static str = "not_found";

    /// 404 — chat room not found.
    pub const ROOM_NOT_FOUND: &'static str = "room_not_found";

    // ---- 409 Conflict ----

    /// 409 — state conflict (generic; prefer a more specific code).
    pub const CONFLICT: &'static str = "conflict";

    /// 409 — username already taken (registration, admin user create).
    pub const USERNAME_TAKEN: &'static str = "username_taken";

    /// 409 — payment already captured (cannot confirm twice).
    pub const PAYMENT_ALREADY_CAPTURED: &'static str = "payment_already_captured";

    // ---- 422 Unprocessable Entity ----

    /// 422 — semantic validation failure (e.g. invalid body shape,
    /// role not in allow-list, field value out of range).
    pub const VALIDATION: &'static str = "validation";

    /// 422 — role string is not in the public-registration allow-list
    /// (`customer` / `technician` only — admin must use the admin
    /// user-create endpoint).
    pub const ROLE_NOT_ALLOWED: &'static str = "role_not_allowed";

    /// 422 — chat message body was empty or too long.
    pub const INVALID_BODY: &'static str = "invalid_body";

    // ---- 429 ----

    /// 429 — per-IP rate limit hit.
    pub const RATE_LIMITED: &'static str = "rate_limited";

    // ---- 5xx ----

    /// 500 — unexpected internal error. Catch-all; specific failures
    /// should use a more targeted code.
    pub const INTERNAL: &'static str = "internal";

    // ---- M20-b: SP_USER_INSERT_FULL (admin user creation) ----
    //
    // These codes mirror the stable string codes emitted by the
    // `dbo.SP_USER_INSERT_FULL` stored procedure (see
    // `crates/domain/src/admin_user.rs`). The handler forwards
    // them verbatim on the wire so admin operators can pattern-
    // match on them. The handler also maps them to the right
    // HTTP status (some 4xx codes here map to 5xx on the wire —
    // see `handlers::admin::sp_insert_full_status`).

    /// 400 — `actor_user_username_guid` is required (admin
    /// creating the user could not be resolved from the JWT).
    pub const ACTOR_REQUIRED: &'static str = "actor_required";

    /// 401 — actor's `user_username_guid` could not be found or is
    /// inactive (deleted / suspended admin tried to create a user).
    pub const ACTOR_NOT_FOUND: &'static str = "actor_not_found";

    /// 403 — actor does not hold the `ADMIN` role (defense-in-depth
    /// check inside the SP — `admin_flag` middleware already gated
    /// this, so a hit here means the role was revoked between JWT
    /// issuance and request handling).
    pub const PERMISSION_DENIED: &'static str = "permission_denied";

    /// 422 — required profile field missing on insert.
    pub const FIRST_NAME_REQUIRED: &'static str = "first_name_required";
    /// 422 — required profile field missing on insert.
    pub const LAST_NAME_REQUIRED: &'static str = "last_name_required";
    /// 422 — required profile field missing on insert.
    pub const EMAIL_REQUIRED: &'static str = "email_required";
    /// 422 — required login field missing on insert.
    pub const USERNAME_REQUIRED: &'static str = "username_required";
    /// 422 — `password_hash` missing on insert (Rust layer bug —
    /// the service always hashes before calling the SP).
    pub const PASSWORD_HASH_REQUIRED: &'static str = "password_hash_required";
    /// 422 — `status` was neither 0 nor 1.
    pub const INVALID_USER_STATUS: &'static str = "invalid_user_status";

    /// 409 — caller-supplied `user_guid` collided with an existing row.
    pub const USER_GUID_EXISTS: &'static str = "user_guid_exists";
    /// 409 — email already in use by another active user.
    pub const EMAIL_TAKEN: &'static str = "email_taken";
    /// 409 — id_card already in use by another active user.
    pub const ID_CARD_TAKEN: &'static str = "id_card_taken";

    /// 422 — `country_guid` required when `is_foreign = 1`.
    pub const COUNTRY_REQUIRED: &'static str = "country_required";
    /// 422 — `country_guid` not found in master_country.
    pub const COUNTRY_NOT_FOUND: &'static str = "country_not_found";
    /// 422 — `company_guid` required when `is_customer_company = 1`.
    pub const COMPANY_REQUIRED: &'static str = "company_required";
    /// 422 — `company_guid` not found in company.
    pub const COMPANY_NOT_FOUND: &'static str = "company_not_found";
    /// 422 — `department_guid` not found or inactive.
    pub const DEPARTMENT_NOT_FOUND: &'static str = "department_not_found";
    /// 422 — `department_team_guid` not found or inactive.
    pub const DEPARTMENT_TEAM_NOT_FOUND: &'static str = "department_team_not_found";
    /// 422 — `department_team_guid` does not belong to `department_guid`.
    pub const DEPARTMENT_TEAM_MISMATCH: &'static str = "department_team_mismatch";
    /// 422 — `position_guid` not found or inactive.
    pub const POSITION_NOT_FOUND: &'static str = "position_not_found";
    /// 422 — `salary_amount` is negative.
    pub const INVALID_SALARY: &'static str = "invalid_salary";
    /// 422 — a working day has `is_working = 1` but start/end time missing.
    pub const WORK_TIME_REQUIRED: &'static str = "work_time_required";

    /// 500 — role `ADMIN` not seeded in master table (operator
    /// must run the seed migration before this endpoint works).
    pub const ADMIN_ROLE_NOT_FOUND: &'static str = "admin_role_not_found";
    /// 500 — role `EMPLOYEE` not seeded in master table.
    pub const EMPLOYEE_ROLE_NOT_FOUND: &'static str = "employee_role_not_found";

    /// 404 — M22-b: `SP_USER_UPDATE_FULL` rejected the update
    /// because the supplied `user_guid` doesn't resolve to a
    /// non-deleted `[user]` row. Symmetric to the `not_found`
    /// generic code but carries the admin-namespace semantics
    /// so the admin UI can branch on it specifically.
    pub const USER_NOT_FOUND: &'static str = "user_not_found";
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The full catalog as a sorted slice. Adding a new code
    /// requires extending this list AND the array below in
    /// `all_codes_are_snake_case_lowercase` so the test catches
    /// duplicates / format drift.
    const CATALOG: &[&str] = &[
        ErrorCode::BAD_REQUEST,
        ErrorCode::IDEMPOTENCY_KEY_REQUIRED,
        ErrorCode::UNAUTHORIZED,
        ErrorCode::INVALID_TOKEN,
        ErrorCode::TOKEN_EXPIRED,
        ErrorCode::REFRESH_INVALID,
        ErrorCode::FORBIDDEN,
        ErrorCode::ADMIN_REQUIRED,
        ErrorCode::NOT_A_PARTICIPANT,
        ErrorCode::NOT_FOUND,
        ErrorCode::ROOM_NOT_FOUND,
        ErrorCode::CONFLICT,
        ErrorCode::USERNAME_TAKEN,
        ErrorCode::PAYMENT_ALREADY_CAPTURED,
        ErrorCode::VALIDATION,
        ErrorCode::ROLE_NOT_ALLOWED,
        ErrorCode::INVALID_BODY,
        ErrorCode::RATE_LIMITED,
        ErrorCode::INTERNAL,
        // M20-b: SP_USER_INSERT_FULL codes
        ErrorCode::ACTOR_REQUIRED,
        ErrorCode::ACTOR_NOT_FOUND,
        ErrorCode::PERMISSION_DENIED,
        ErrorCode::FIRST_NAME_REQUIRED,
        ErrorCode::LAST_NAME_REQUIRED,
        ErrorCode::EMAIL_REQUIRED,
        ErrorCode::USERNAME_REQUIRED,
        ErrorCode::PASSWORD_HASH_REQUIRED,
        ErrorCode::INVALID_USER_STATUS,
        ErrorCode::USER_GUID_EXISTS,
        ErrorCode::EMAIL_TAKEN,
        ErrorCode::ID_CARD_TAKEN,
        ErrorCode::COUNTRY_REQUIRED,
        ErrorCode::COUNTRY_NOT_FOUND,
        ErrorCode::COMPANY_REQUIRED,
        ErrorCode::COMPANY_NOT_FOUND,
        ErrorCode::DEPARTMENT_NOT_FOUND,
        ErrorCode::DEPARTMENT_TEAM_NOT_FOUND,
        ErrorCode::DEPARTMENT_TEAM_MISMATCH,
        ErrorCode::POSITION_NOT_FOUND,
        ErrorCode::INVALID_SALARY,
        ErrorCode::WORK_TIME_REQUIRED,
        ErrorCode::ADMIN_ROLE_NOT_FOUND,
        ErrorCode::EMPLOYEE_ROLE_NOT_FOUND,
        ErrorCode::USER_NOT_FOUND,
    ];

    #[test]
    fn codes_are_unique() {
        let mut sorted = CATALOG.to_vec();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), CATALOG.len(), "duplicate code in catalog");
    }

    #[test]
    fn codes_are_snake_case_lowercase() {
        for code in CATALOG {
            assert!(
                code.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "code `{code}` must be snake_case lowercase ASCII (no spaces, hyphens, or uppercase)"
            );
        }
    }

    #[test]
    fn codes_have_no_trailing_underscore() {
        for code in CATALOG {
            assert!(
                !code.starts_with('_') && !code.ends_with('_'),
                "code `{code}` must not start or end with underscore"
            );
        }
    }

    #[test]
    fn codes_have_reasonable_length() {
        // Not enforced as a hard rule, just a sanity check —
        // shorter is better for log grep-ability and HTTP header
        // economy (the code is JSON-serialised in every error
        // response).
        for code in CATALOG {
            assert!(
                code.len() <= 40,
                "code `{code}` is too long ({}) — keep it under 40 chars",
                code.len()
            );
        }
    }
}
