//! `LocalizedError` — the bridge between typed domain errors and the
//! i18n catalog.
//!
//! ## Why a trait?
//!
//! The domain layer expresses failures as typed `thiserror` enums
//! (`AuthError`, `RepoError`, `ChatError`, `PaymentError`, ...). The
//! `#[error("...")]` attribute on each variant produces a stable
//! English `Display` for logs and tests, but the user-facing API
//! response needs to be in the request's `Accept-Language` locale.
//!
//! `LocalizedError` is the contract that maps each error to a
//! translation key plus a list of positional arguments. The HTTP
//! layer (which is allowed to know about i18n) calls
//! [`LocalizedError::l10n_message`] with the per-request locale and
//! a [`crate::traits::translation::TranslationRepository`] to render
//! the final string.
//!
//! ## Two-step rendering
//!
//! 1. **Key resolution**: each variant returns a stable
//!    `dot.path.like.this` (e.g. `err_repo.not_found`).
//! 2. **Argument substitution**: the variant returns the strings that
//!    fill the key's `{0}`, `{1}`, ... placeholders.
//!
//! The two pieces travel separately so a translation author can
//! reorder placeholders in a locale without breaking the trait
//! contract.
//!
//! ## Trait placement
//!
//! Lives in `domain` (not `common`) so the port is adjacent to the
//! entities it serves. `common::i18n` only consumes it.

use crate::traits::chat::ChatRepoError;
use crate::traits::payment::PaymentRepoError;
use crate::traits::user::RepoError;

use crate::auth::AuthError;
use crate::chat::ChatError;
use crate::payment::PaymentError;

/// A domain error that can render a locale-aware message.
///
/// Implementations are deliberately **sync**: the trait only returns
/// a translation key and a list of argument strings. The actual
/// translation lookup (which may hit a DB) is the caller's job — see
/// [`kokkak_common::i18n::tr_with_repo`].
pub trait LocalizedError {
    /// Stable dotted-path key (e.g. `"err_repo.not_found"`). The
    /// translation catalog must contain this key in every supported
    /// locale.
    fn l10n_key(&self) -> &'static str;

    /// Positional arguments that fill the key's `{0}`, `{1}`, ...
    /// placeholders. Empty when the key has no placeholders.
    fn l10n_args(&self) -> Vec<String>;
}

impl LocalizedError for AuthError {
    fn l10n_key(&self) -> &'static str {
        match self {
            Self::InvalidCredentials => "err_auth.invalid_credentials",
            Self::TokenExpired => "err_auth.token_expired",
            Self::InvalidToken(_) => "err_auth.invalid_token",
            Self::Forbidden(_) => "err_auth.forbidden",
            // M14: renamed from `err_auth.email_taken` to reflect
            // NEW_DB's username-based login identifier.
            Self::UsernameTaken => "err_auth.username_taken",
            Self::Validation(_) => "err_auth.validation",
            Self::Backend(_) => "err_auth.backend",
            // ponytail: matches the `err.*` rename in api/error.rs
            // l10n_key_for_app_error (2026-07-01). yml catalogs use
            // `err.rate_limited` not `err_general.rate_limited`.
            Self::RateLimited(_) => "err.rate_limited",
        }
    }

    fn l10n_args(&self) -> Vec<String> {
        match self {
            Self::InvalidToken(d) | Self::Forbidden(d) | Self::Validation(d) | Self::Backend(d) => {
                vec![d.clone()]
            }
            _ => vec![],
        }
    }
}

impl LocalizedError for RepoError {
    fn l10n_key(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "err_repo.not_found",
            Self::Conflict(_) => "err_repo.conflict",
            Self::Backend(_) => "err_repo.backend",
        }
    }

    fn l10n_args(&self) -> Vec<String> {
        match self {
            Self::NotFound(d) | Self::Conflict(d) | Self::Backend(d) => vec![d.clone()],
        }
    }
}

impl LocalizedError for ChatError {
    fn l10n_key(&self) -> &'static str {
        match self {
            Self::NotParticipant(_) => "err_chat.not_participant",
            Self::RoomNotFound(_) => "err_chat.room_not_found",
            Self::InvalidBody(_) => "err_chat.invalid_body",
            Self::Backend(_) => "err_chat.backend",
        }
    }

    fn l10n_args(&self) -> Vec<String> {
        match self {
            Self::NotParticipant(id) => vec![id.to_string()],
            Self::RoomNotFound(id) => vec![id.to_string()],
            Self::InvalidBody(d) | Self::Backend(d) => vec![d.clone()],
        }
    }
}

impl LocalizedError for ChatRepoError {
    fn l10n_key(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "err_chat_repo.not_found",
            Self::Backend(_) => "err_chat_repo.backend",
        }
    }

    fn l10n_args(&self) -> Vec<String> {
        match self {
            Self::NotFound(d) | Self::Backend(d) => vec![d.clone()],
        }
    }
}

impl LocalizedError for PaymentError {
    fn l10n_key(&self) -> &'static str {
        match self {
            Self::OrderNotPayable(_) => "err_payment.order_not_payable",
            Self::NotFound(_) => "err_payment.not_found_msg",
            Self::InvalidAmount(_) => "err_payment.invalid_amount",
            Self::Backend(_) => "err_payment.backend",
        }
    }

    fn l10n_args(&self) -> Vec<String> {
        match self {
            Self::OrderNotPayable(id) => vec![id.to_string()],
            Self::NotFound(id) => vec![id.to_string()],
            Self::InvalidAmount(d) | Self::Backend(d) => vec![d.clone()],
        }
    }
}

impl LocalizedError for PaymentRepoError {
    fn l10n_key(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "err_payment_repo.not_found",
            Self::Backend(_) => "err_payment_repo.backend",
        }
    }

    fn l10n_args(&self) -> Vec<String> {
        match self {
            Self::NotFound(d) | Self::Backend(d) => vec![d.clone()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn auth_error_keys_match_catalog() {
        assert_eq!(
            AuthError::InvalidCredentials.l10n_key(),
            "err_auth.invalid_credentials"
        );
        // M14: renamed.
        assert_eq!(
            AuthError::UsernameTaken.l10n_key(),
            "err_auth.username_taken"
        );
        assert!(AuthError::InvalidCredentials.l10n_args().is_empty());
    }

    #[test]
    fn auth_error_invalid_token_carries_reason() {
        let e = AuthError::InvalidToken("bad sig".into());
        assert_eq!(e.l10n_key(), "err_auth.invalid_token");
        assert_eq!(e.l10n_args(), vec!["bad sig".to_string()]);
    }

    #[test]
    fn repo_error_keys_match_catalog() {
        assert_eq!(
            RepoError::NotFound("u".into()).l10n_key(),
            "err_repo.not_found"
        );
        assert_eq!(
            RepoError::Conflict("dup".into()).l10n_key(),
            "err_repo.conflict"
        );
        assert_eq!(
            RepoError::Backend("db down".into()).l10n_key(),
            "err_repo.backend"
        );
    }

    #[test]
    fn chat_error_room_id_arg_is_uuid() {
        let id = Uuid::nil();
        let e = ChatError::NotParticipant(id);
        assert_eq!(e.l10n_key(), "err_chat.not_participant");
        assert_eq!(e.l10n_args(), vec![Uuid::nil().to_string()]);
    }

    #[test]
    fn payment_error_order_not_payable_carries_id() {
        let id = Uuid::nil();
        let e = PaymentError::OrderNotPayable(id);
        assert_eq!(e.l10n_key(), "err_payment.order_not_payable");
        assert_eq!(e.l10n_args(), vec![id.to_string()]);
    }

    #[test]
    fn chat_repo_error_backend_carries_message() {
        let e = ChatRepoError::Backend("boom".into());
        assert_eq!(e.l10n_key(), "err_chat_repo.backend");
        assert_eq!(e.l10n_args(), vec!["boom".to_string()]);
    }
}
