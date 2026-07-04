

use std::fmt;

use axum::response::{IntoResponse, Response};
use kokkak_common::error::AppError;
use kokkak_common::i18n::{current_locale, tr_with_repo};
use kokkak_domain::{
    AuthError, ChatError, ChatRepoError, PaymentError, PaymentRepoError, RepoError,
};

use crate::state::AppState;

#[derive(Debug)]
pub struct ApiError(pub AppError);

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ApiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

impl From<AppError> for ApiError {
    fn from(inner: AppError) -> Self {
        Self(inner)
    }
}

impl From<AuthError> for ApiError {
    fn from(e: AuthError) -> Self {
        Self(match e {
            AuthError::InvalidCredentials => AppError::Unauthorized,
            AuthError::TokenExpired => AppError::TokenExpired,
            AuthError::InvalidToken(reason) => AppError::InvalidToken(reason),
            AuthError::Forbidden(reason) => AppError::Forbidden(reason),

            AuthError::UsernameTaken => AppError::UsernameTaken,
            AuthError::Validation(msg) => AppError::Validation(msg),
            AuthError::Backend(msg) => AppError::Internal(msg),
            AuthError::RateLimited(secs) => AppError::RateLimited {
                retry_after_secs: secs,
            },
        })
    }
}

impl From<RepoError> for ApiError {
    fn from(e: RepoError) -> Self {
        Self(match e {
            RepoError::NotFound(what) => AppError::NotFound(what),
            RepoError::Conflict(what) => AppError::Conflict(what),
            RepoError::Backend(what) => AppError::Internal(what),
        })
    }
}

impl From<ChatError> for ApiError {
    fn from(e: ChatError) -> Self {
        Self(match e {
            ChatError::NotParticipant(_) => AppError::Forbidden("not a chat participant".into()),
            ChatError::RoomNotFound(_) => AppError::NotFound("chat room".into()),
            ChatError::InvalidBody(msg) => AppError::Validation(msg),
            ChatError::Backend(msg) => AppError::Internal(msg),
        })
    }
}

impl From<ChatRepoError> for ApiError {
    fn from(e: ChatRepoError) -> Self {
        Self(match e {
            ChatRepoError::NotFound(what) => AppError::NotFound(what),
            ChatRepoError::Backend(what) => AppError::Internal(what),
        })
    }
}

impl From<PaymentError> for ApiError {
    fn from(e: PaymentError) -> Self {
        Self(match e {
            PaymentError::OrderNotPayable(id) => {
                AppError::Conflict(format!("order {id} is not payable"))
            }
            PaymentError::NotFound(id) => AppError::NotFound(format!("payment {id}")),
            PaymentError::InvalidAmount(msg) => AppError::Validation(msg),
            PaymentError::Backend(msg) => AppError::Internal(msg),
        })
    }
}

impl From<PaymentRepoError> for ApiError {
    fn from(e: PaymentRepoError) -> Self {
        Self(match e {
            PaymentRepoError::NotFound(what) => AppError::NotFound(what),
            PaymentRepoError::Backend(what) => AppError::Internal(what),
        })
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        self.0.into_response()
    }
}

pub trait IntoLocalizedResponse {

    fn into_localized_response(
        self,
        state: &AppState,
    ) -> impl std::future::Future<Output = Response> + Send;
}

impl IntoLocalizedResponse for ApiError {
    async fn into_localized_response(self, state: &AppState) -> Response {
        self.0.into_localized_response(state).await
    }
}

impl IntoLocalizedResponse for AppError {
    async fn into_localized_response(self, state: &AppState) -> Response {

        if let AppError::Localized { .. } = &self {
            return self.into_response();
        }
        let key = l10n_key_for_app_error(&self);
        let args: Vec<String> = l10n_args_for_app_error(&self);
        let args_ref: Vec<&str> = args.iter().map(String::as_str).collect();
        let locale = current_locale();
        let message = tr_with_repo(&*state.translation, &locale, key, &args_ref).await;
        self.with_message(message).into_response()
    }
}

fn l10n_key_for_app_error(err: &AppError) -> &'static str {

    match err {
        AppError::BadRequest(_) => "err.bad_request",
        AppError::Unauthorized => "err_auth.invalid_credentials",
        AppError::InvalidToken(_) => "err_auth.invalid_token",
        AppError::TokenExpired => "err_auth.token_expired",
        AppError::Forbidden(_) => "err_auth.forbidden",
        AppError::AdminRequired => "err_auth.admin_required",
        AppError::NotFound(_) => "err_repo.not_found",
        AppError::Conflict(_) => "err_repo.conflict",
        AppError::UsernameTaken => "err_auth.username_taken",
        AppError::Validation(_) => "err_auth.validation",
        AppError::RoleNotAllowed(_) => "err_auth.role_not_allowed",
        AppError::RateLimited { .. } => "err.rate_limited",
        AppError::Internal(_) => "err.internal",

        AppError::Localized { .. } => "",
    }
}

fn l10n_args_for_app_error(err: &AppError) -> Vec<String> {
    match err {
        AppError::BadRequest(s)
        | AppError::InvalidToken(s)
        | AppError::Forbidden(s)
        | AppError::NotFound(s)
        | AppError::Conflict(s)
        | AppError::Validation(s)
        | AppError::RoleNotAllowed(s)
        | AppError::Internal(s) => vec![s.clone()],
        AppError::Unauthorized
        | AppError::TokenExpired
        | AppError::AdminRequired
        | AppError::UsernameTaken
        | AppError::RateLimited { .. }
        | AppError::Localized { .. } => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kokkak_common::i18n::init_i18n;
    use kokkak_domain::LocalizedError;

    #[test]
    fn from_auth_error_maps_to_localizable_variant() {

        let pairs: Vec<(AuthError, &str)> = vec![
            (
                AuthError::InvalidCredentials,
                "err_auth.invalid_credentials",
            ),
            (AuthError::TokenExpired, "err_auth.token_expired"),
            (
                AuthError::InvalidToken("bad".into()),
                "err_auth.invalid_token",
            ),
            (AuthError::Forbidden("nope".into()), "err_auth.forbidden"),
            (AuthError::UsernameTaken, "err_auth.username_taken"),
            (AuthError::Validation("x".into()), "err_auth.validation"),

        ];
        for (src, expected_key) in pairs {
            let api_err: ApiError = ApiError::from(src);
            let inner = &api_err.0;
            assert_eq!(
                l10n_key_for_app_error(inner),
                expected_key,
                "AuthError:: → {inner:?} maps to wrong key"
            );
        }

        let api_err: ApiError = ApiError::from(AuthError::Backend("x".into()));
        assert_eq!(l10n_key_for_app_error(&api_err.0), "err.internal");
    }

    #[test]
    fn i18n_key_drift_2026_07_01_locked_in() {
        let cases: Vec<(AppError, &str)> = vec![
            (AppError::BadRequest("x".into()), "err.bad_request"),
            (
                AppError::RateLimited {
                    retry_after_secs: 1,
                },
                "err.rate_limited",
            ),
            (AppError::Internal("x".into()), "err.internal"),
            (
                AppError::RoleNotAllowed("x".into()),
                "err_auth.role_not_allowed",
            ),
            (AppError::NotFound("x".into()), "err_repo.not_found"),
        ];
        for (variant, expected) in cases {
            assert_eq!(
                l10n_key_for_app_error(&variant),
                expected,
                "key drift for {variant:?}"
            );
        }
    }

    #[test]
    fn auth_error_rate_limited_key_matches_api_layer() {
        assert_eq!(AuthError::RateLimited(60).l10n_key(), "err.rate_limited");
    }

    #[test]
    fn all_login_failure_modes_produce_byte_identical_unauthorized_response() {

        let scenarios: Vec<(&'static str, AuthError)> = vec![

            (
                "user_not_found",
                AuthError::InvalidCredentials,
            ),

            ("wrong_password", AuthError::InvalidCredentials),

            ("account_suspended", AuthError::InvalidCredentials),

            ("account_deleted", AuthError::InvalidCredentials),

            ("account_pending", AuthError::InvalidCredentials),

        ];

        for (label, src) in scenarios {
            let api: ApiError = ApiError::from(src);
            assert_eq!(
                api.0,
                AppError::Unauthorized,
                "{label}: login failure must collapse into AppError::Unauthorized"
            );
            assert_eq!(
                api.0.status().as_u16(),
                401,
                "{label}: HTTP status must be 401"
            );
            assert_eq!(
                api.0.code(),
                "unauthorized",
                "{label}: error code must be the generic `unauthorized`"
            );
            assert_eq!(
                l10n_key_for_app_error(&api.0),
                "err_auth.invalid_credentials",
                "{label}: i18n key must be the generic `err_auth.invalid_credentials`"
            );
            assert!(
                l10n_args_for_app_error(&api.0).is_empty(),
                "{label}: i18n args must be empty (no leaked context)"
            );
        }
    }

    #[test]
    fn from_repo_error_maps_to_app_error() {
        let pairs: Vec<(RepoError, &str)> = vec![
            (RepoError::NotFound("u".into()), "not_found"),
            (RepoError::Conflict("dup".into()), "conflict"),
            (RepoError::Backend("db".into()), "internal"),
        ];
        for (src, expected_code) in pairs {
            let api_err: ApiError = ApiError::from(src);
            assert_eq!(api_err.0.code(), expected_code);
        }
    }

    #[test]
    fn from_chat_error_maps_to_app_error() {
        use uuid::Uuid;
        let pairs: Vec<(ChatError, u16)> = vec![
            (ChatError::NotParticipant(Uuid::nil()), 403),
            (ChatError::RoomNotFound(Uuid::nil()), 404),
            (ChatError::InvalidBody("empty".into()), 422),
            (ChatError::Backend("db".into()), 500),
        ];
        for (src, expected_status) in pairs {
            let api_err: ApiError = ApiError::from(src);
            assert_eq!(
                api_err.0.status().as_u16(),
                expected_status,
                "ChatError:: → wrong status"
            );
        }
    }

    #[test]
    fn from_payment_error_maps_to_app_error() {
        use uuid::Uuid;
        let pairs: Vec<(PaymentError, u16)> = vec![
            (PaymentError::OrderNotPayable(Uuid::nil()), 409),
            (PaymentError::NotFound(Uuid::nil()), 404),
            (PaymentError::InvalidAmount("0".into()), 422),
            (PaymentError::Backend("db".into()), 500),
        ];
        for (src, expected_status) in pairs {
            let api_err: ApiError = ApiError::from(src);
            assert_eq!(
                api_err.0.status().as_u16(),
                expected_status,
                "PaymentError:: → wrong status"
            );
        }
    }

    #[test]
    fn l10n_args_match_auth_error_payload() {
        let e = AuthError::InvalidToken("signature mismatch".into());
        let api: ApiError = ApiError::from(e.clone());
        assert_eq!(l10n_args_for_app_error(&api.0), e.l10n_args());

        let e = AuthError::Forbidden("not your room".into());
        let api: ApiError = ApiError::from(e.clone());
        assert_eq!(l10n_args_for_app_error(&api.0), e.l10n_args());

        let e = AuthError::Validation("username must not be empty".into());
        let api: ApiError = ApiError::from(e.clone());
        assert_eq!(l10n_args_for_app_error(&api.0), e.l10n_args());
    }

    #[test]
    fn from_app_error_round_trip() {
        let original = AppError::Validation("x".into());
        let api_err: ApiError = ApiError::from(original);
        assert_eq!(api_err.0.status().as_u16(), 422);
        assert_eq!(api_err.0.code(), "validation");
    }

    #[test]
    fn api_error_display_delegates_to_inner() {
        let api_err = ApiError(AppError::NotFound("widget".into()));
        assert_eq!(api_err.to_string(), "not found: widget");
    }

    #[test]
    fn api_error_source_is_inner_app_error() {
        use std::error::Error;
        let api_err = ApiError(AppError::Internal("boom".into()));
        let src = api_err.source().expect("source must exist");
        assert_eq!(src.to_string(), "internal: boom");
    }

    #[test]
    fn localized_app_error_passthrough() {
        let err = AppError::Localized {
            status: axum::http::StatusCode::BAD_REQUEST,
            code: "bad_request",
            message: "ข้อความเดิม".into(),
        };
        assert_eq!(l10n_key_for_app_error(&err), "");
        assert_eq!(err.status(), axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(err.body().message, "ข้อความเดิม");
    }

    #[test]
    fn init_i18n_succeeds_for_translation_tests() {

        #[allow(clippy::let_unit_value)]
        let _ = init_i18n("en");
    }
}
