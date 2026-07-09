use chrono::Utc;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use uuid::Uuid;

use kokkak_common::config::AuthSettings;
use kokkak_domain::{AuthError, Claims, Role, TokenKind};

#[derive(Clone)]
pub struct JwtService {
    encoding: EncodingKey,
    decoding: DecodingKey,
    validation: Validation,
    issuer: String,
    access_ttl_secs: i64,
    refresh_ttl_secs: i64,
}

impl std::fmt::Debug for JwtService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwtService")
            .field("issuer", &self.issuer)
            .field("access_ttl_secs", &self.access_ttl_secs)
            .field("refresh_ttl_secs", &self.refresh_ttl_secs)
            .finish_non_exhaustive()
    }
}

impl JwtService {
    pub fn new(settings: &AuthSettings) -> Result<Self, AuthError> {
        if settings.jwt_secret.is_empty() {
            return Err(AuthError::Backend(
                "jwt_secret is empty — set KOKKAK_AUTH__JWT_SECRET".into(),
            ));
        }
        Ok(Self {
            encoding: EncodingKey::from_secret(settings.jwt_secret.as_bytes()),
            decoding: DecodingKey::from_secret(settings.jwt_secret.as_bytes()),
            validation: Validation::new(jsonwebtoken::Algorithm::HS256),
            issuer: settings.issuer.clone(),
            access_ttl_secs: settings.access_ttl_secs,
            refresh_ttl_secs: settings.refresh_ttl_secs,
        })
    }

    pub fn issue_access(
        &self,
        user_id: Uuid,
        roles: &[Role],
        scope: &str,
    ) -> Result<(String, String), AuthError> {
        self.issue(
            user_id,
            roles,
            scope,
            TokenKind::Access,
            self.access_ttl_secs,
        )
    }

    pub fn issue_refresh(
        &self,
        user_id: Uuid,
        roles: &[Role],
        scope: &str,
    ) -> Result<(String, String), AuthError> {
        self.issue(
            user_id,
            roles,
            scope,
            TokenKind::Refresh,
            self.refresh_ttl_secs,
        )
    }

    fn issue(
        &self,
        user_id: Uuid,
        roles: &[Role],
        scope: &str,
        kind: TokenKind,
        ttl_secs: i64,
    ) -> Result<(String, String), AuthError> {
        let now = Utc::now().timestamp();
        let jti = Uuid::new_v4().to_string();
        let claims = Claims {
            sub: user_id,
            iss: self.issuer.clone(),
            iat: now,
            exp: now + ttl_secs,
            kind,
            roles: roles.to_vec(),
            scope: scope.to_string(),
            jti: jti.clone(),
        };
        let token = encode(
            &Header::new(jsonwebtoken::Algorithm::HS256),
            &claims,
            &self.encoding,
        )
        .map_err(|e| AuthError::Backend(format!("jwt encode: {e}")))?;
        Ok((token, jti))
    }

    pub fn verify(&self, token: &str) -> Result<Claims, AuthError> {
        let data =
            decode::<Claims>(token, &self.decoding, &self.validation).map_err(|e| {
                match e.kind() {
                    jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::TokenExpired,
                    _ => AuthError::InvalidToken(e.to_string()),
                }
            })?;

        if data.claims.iss != self.issuer {
            return Err(AuthError::InvalidToken("issuer mismatch".into()));
        }
        Ok(data.claims)
    }

    pub fn access_ttl_secs(&self) -> i64 {
        self.access_ttl_secs
    }

    pub fn refresh_ttl_secs(&self) -> i64 {
        self.refresh_ttl_secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(secret: &str) -> AuthSettings {
        AuthSettings {
            jwt_secret: secret.into(),
            issuer: "kokkak-test".into(),
            access_ttl_secs: 900,
            refresh_ttl_secs: 3600,
        }
    }

    #[test]
    fn new_fails_with_empty_secret() {
        let mut s = settings("x");
        s.jwt_secret.clear();
        let err = JwtService::new(&s).unwrap_err();
        assert!(matches!(err, AuthError::Backend(_)));
    }

    #[test]
    fn issue_and_verify_round_trip() {
        let s = settings("test-secret-please-change-me");
        let svc = JwtService::new(&s).unwrap();
        let user_id = Uuid::new_v4();
        let (token, jti) = svc
            .issue_access(user_id, &[Role::Customer], "mobile")
            .unwrap();
        assert!(!jti.is_empty());
        let claims = svc.verify(&token).unwrap();
        assert_eq!(claims.sub, user_id);
        assert_eq!(claims.kind, TokenKind::Access);
        assert_eq!(claims.scope, "mobile");
        assert_eq!(claims.roles, vec![Role::Customer]);
        assert_eq!(claims.jti, jti);
    }

    #[test]
    fn refresh_and_access_distinguished_by_kind() {
        let s = settings("test-secret-please-change-me");
        let svc = JwtService::new(&s).unwrap();
        let user_id = Uuid::new_v4();
        let (access, _) = svc.issue_access(user_id, &[Role::Admin], "admin").unwrap();
        let (refresh, _) = svc.issue_refresh(user_id, &[Role::Admin], "admin").unwrap();
        let a = svc.verify(&access).unwrap();
        let r = svc.verify(&refresh).unwrap();
        assert_eq!(a.kind, TokenKind::Access);
        assert_eq!(r.kind, TokenKind::Refresh);
    }

    #[test]
    fn tampered_token_fails() {
        let s = settings("test-secret-please-change-me");
        let svc = JwtService::new(&s).unwrap();
        let (token, _) = svc
            .issue_access(Uuid::new_v4(), &[Role::Customer], "x")
            .unwrap();
        let mut tampered = token.clone();

        let last = tampered.pop().unwrap();
        tampered.push(if last == 'A' { 'B' } else { 'A' });
        let err = svc.verify(&tampered).unwrap_err();
        assert!(matches!(err, AuthError::InvalidToken(_)));
    }

    #[test]
    fn wrong_issuer_fails() {
        let s1 = settings("test-secret");
        let s2 = AuthSettings {
            issuer: "other-issuer".into(),
            ..s1.clone()
        };
        let svc1 = JwtService::new(&s1).unwrap();
        let svc2 = JwtService::new(&s2).unwrap();
        let (token, _) = svc1
            .issue_access(Uuid::new_v4(), &[Role::Customer], "x")
            .unwrap();
        let err = svc2.verify(&token).unwrap_err();
        assert!(matches!(err, AuthError::InvalidToken(_)));
    }
}
