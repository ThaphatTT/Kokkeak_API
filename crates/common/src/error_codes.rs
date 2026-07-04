

pub struct ErrorCode;

impl ErrorCode {

    pub const BAD_REQUEST: &'static str = "bad_request";

    pub const IDEMPOTENCY_KEY_REQUIRED: &'static str = "idempotency_key_required";

    pub const UNAUTHORIZED: &'static str = "unauthorized";

    pub const INVALID_TOKEN: &'static str = "invalid_token";

    pub const TOKEN_EXPIRED: &'static str = "token_expired";

    pub const REFRESH_INVALID: &'static str = "refresh_invalid";

    pub const FORBIDDEN: &'static str = "forbidden";

    pub const ADMIN_REQUIRED: &'static str = "admin_required";

    pub const NOT_A_PARTICIPANT: &'static str = "not_a_participant";

    pub const NOT_FOUND: &'static str = "not_found";

    pub const ROOM_NOT_FOUND: &'static str = "room_not_found";

    pub const CONFLICT: &'static str = "conflict";

    pub const USERNAME_TAKEN: &'static str = "username_taken";

    pub const PAYMENT_ALREADY_CAPTURED: &'static str = "payment_already_captured";

    pub const VALIDATION: &'static str = "validation";

    pub const ROLE_NOT_ALLOWED: &'static str = "role_not_allowed";

    pub const INVALID_BODY: &'static str = "invalid_body";

    pub const RATE_LIMITED: &'static str = "rate_limited";

    pub const INTERNAL: &'static str = "internal";

    pub const ACTOR_REQUIRED: &'static str = "actor_required";

    pub const ACTOR_NOT_FOUND: &'static str = "actor_not_found";

    pub const PERMISSION_DENIED: &'static str = "permission_denied";

    pub const FIRST_NAME_REQUIRED: &'static str = "first_name_required";

    pub const LAST_NAME_REQUIRED: &'static str = "last_name_required";

    pub const EMAIL_REQUIRED: &'static str = "email_required";

    pub const USERNAME_REQUIRED: &'static str = "username_required";

    pub const PASSWORD_HASH_REQUIRED: &'static str = "password_hash_required";

    pub const INVALID_USER_STATUS: &'static str = "invalid_user_status";

    pub const USER_GUID_EXISTS: &'static str = "user_guid_exists";

    pub const EMAIL_TAKEN: &'static str = "email_taken";

    pub const ID_CARD_TAKEN: &'static str = "id_card_taken";

    pub const COUNTRY_REQUIRED: &'static str = "country_required";

    pub const COUNTRY_NOT_FOUND: &'static str = "country_not_found";

    pub const COMPANY_REQUIRED: &'static str = "company_required";

    pub const COMPANY_NOT_FOUND: &'static str = "company_not_found";

    pub const DEPARTMENT_NOT_FOUND: &'static str = "department_not_found";

    pub const DEPARTMENT_TEAM_NOT_FOUND: &'static str = "department_team_not_found";

    pub const DEPARTMENT_TEAM_MISMATCH: &'static str = "department_team_mismatch";

    pub const POSITION_NOT_FOUND: &'static str = "position_not_found";

    pub const INVALID_SALARY: &'static str = "invalid_salary";

    pub const WORK_TIME_REQUIRED: &'static str = "work_time_required";

    pub const ADMIN_ROLE_NOT_FOUND: &'static str = "admin_role_not_found";

    pub const EMPLOYEE_ROLE_NOT_FOUND: &'static str = "employee_role_not_found";

    pub const USER_NOT_FOUND: &'static str = "user_not_found";
}

#[cfg(test)]
mod tests {
    use super::*;

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

        for code in CATALOG {
            assert!(
                code.len() <= 40,
                "code `{code}` is too long ({}) — keep it under 40 chars",
                code.len()
            );
        }
    }
}
