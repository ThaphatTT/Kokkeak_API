

use crate::traits::chat::ChatRepoError;
use crate::traits::payment::PaymentRepoError;
use crate::traits::user::RepoError;

use crate::auth::AuthError;
use crate::chat::ChatError;
use crate::payment::PaymentError;

pub trait LocalizedError {

    fn l10n_key(&self) -> &'static str;

    fn l10n_args(&self) -> Vec<String>;
}

impl LocalizedError for AuthError {
    fn l10n_key(&self) -> &'static str {
        match self {
            Self::InvalidCredentials => "err_auth.invalid_credentials",
            Self::TokenExpired => "err_auth.token_expired",
            Self::InvalidToken(_) => "err_auth.invalid_token",
            Self::Forbidden(_) => "err_auth.forbidden",

            Self::UsernameTaken => "err_auth.username_taken",
            Self::Validation(_) => "err_auth.validation",
            Self::Backend(_) => "err_auth.backend",

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
