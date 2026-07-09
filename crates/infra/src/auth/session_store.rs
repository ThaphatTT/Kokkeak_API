use async_trait::async_trait;
use deadpool_redis::Pool;
use redis::AsyncCommands;
use uuid::Uuid;

use kokkak_domain::{AuthError, CreateSession, SessionInfo, SessionStore};

#[derive(Clone)]
pub struct RedisSessionStore {
    pool: Pool,
    key_prefix: String,
}

impl RedisSessionStore {
    pub fn new(pool: Pool) -> Self {
        Self {
            pool,
            key_prefix: "kokkak:v1:auth:session".into(),
        }
    }

    fn key(&self, user_id: Uuid, jti: &str) -> String {
        format!("{}:{}:{}", self.key_prefix, user_id, jti)
    }

    fn pattern(&self, user_id: Uuid) -> String {
        format!("{}:{}:*", self.key_prefix, user_id)
    }
}

#[async_trait]
impl SessionStore for RedisSessionStore {
    async fn create(&self, session: &CreateSession) -> Result<(), AuthError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AuthError::Backend(format!("redis pool: {e}")))?;
        let value = serde_json::json!({
            "jti": session.jti,
            "user_id": session.user_id.to_string(),
            "scope": session.scope,
            "device": session.device,
            "ip": session.ip,
            "created_at": chrono::Utc::now().timestamp(),
        });
        let _: () = redis::cmd("SET")
            .arg(self.key(session.user_id, &session.jti))
            .arg(value.to_string())
            .arg("EX")
            .arg(session.ttl_secs)
            .query_async(&mut *conn)
            .await
            .map_err(|e| AuthError::Backend(format!("redis set: {e}")))?;
        Ok(())
    }

    async fn get(&self, user_id: Uuid, jti: &str) -> Result<Option<SessionInfo>, AuthError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AuthError::Backend(format!("redis pool: {e}")))?;
        let v: Option<String> = redis::cmd("GET")
            .arg(self.key(user_id, jti))
            .query_async(&mut *conn)
            .await
            .map_err(|e| AuthError::Backend(format!("redis get: {e}")))?;
        match v {
            Some(s) => {
                let v: serde_json::Value = serde_json::from_str(&s)
                    .map_err(|e| AuthError::Backend(format!("session json: {e}")))?;
                Ok(Some(SessionInfo {
                    jti: v["jti"].as_str().unwrap_or_default().to_string(),
                    user_id,
                    scope: v["scope"].as_str().unwrap_or_default().to_string(),
                    device: v["device"].as_str().unwrap_or_default().to_string(),
                    ip: v["ip"].as_str().unwrap_or_default().to_string(),
                    created_at: v["created_at"].as_i64().unwrap_or_default(),
                }))
            }
            None => Ok(None),
        }
    }

    async fn revoke(&self, user_id: Uuid, jti: &str) -> Result<(), AuthError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AuthError::Backend(format!("redis pool: {e}")))?;
        let _: () = redis::cmd("DEL")
            .arg(self.key(user_id, jti))
            .query_async(&mut *conn)
            .await
            .map_err(|e| AuthError::Backend(format!("redis del: {e}")))?;
        Ok(())
    }

    async fn revoke_all(&self, user_id: Uuid) -> Result<u64, AuthError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AuthError::Backend(format!("redis pool: {e}")))?;
        let pattern = self.pattern(user_id);
        let mut cursor: u64 = 0;
        let mut deleted: u64 = 0;
        loop {
            let (next_cursor, keys): (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(&pattern)
                .arg("COUNT")
                .arg(100)
                .query_async(&mut *conn)
                .await
                .map_err(|e| AuthError::Backend(format!("redis scan: {e}")))?;
            if !keys.is_empty() {
                let removed: u64 = redis::cmd("DEL")
                    .arg(&keys)
                    .query_async(&mut *conn)
                    .await
                    .map_err(|e| AuthError::Backend(format!("redis del: {e}")))?;
                deleted += removed;
            }
            cursor = next_cursor;
            if cursor == 0 {
                break;
            }
        }
        Ok(deleted)
    }

    async fn list(&self, user_id: Uuid) -> Result<Vec<SessionInfo>, AuthError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AuthError::Backend(format!("redis pool: {e}")))?;
        let pattern = self.pattern(user_id);
        let mut cursor: u64 = 0;
        let mut sessions = Vec::new();
        loop {
            let (next_cursor, keys): (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(&pattern)
                .arg("COUNT")
                .arg(100)
                .query_async(&mut *conn)
                .await
                .map_err(|e| AuthError::Backend(format!("redis scan: {e}")))?;
            for key in &keys {
                let v: Option<String> = redis::cmd("GET")
                    .arg(key)
                    .query_async(&mut *conn)
                    .await
                    .map_err(|e| AuthError::Backend(format!("redis get: {e}")))?;
                if let Some(s) = v {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&s) {
                        sessions.push(SessionInfo {
                            jti: val["jti"].as_str().unwrap_or_default().to_string(),
                            user_id,
                            scope: val["scope"].as_str().unwrap_or_default().to_string(),
                            device: val["device"].as_str().unwrap_or_default().to_string(),
                            ip: val["ip"].as_str().unwrap_or_default().to_string(),
                            created_at: val["created_at"].as_i64().unwrap_or_default(),
                        });
                    }
                }
            }
            cursor = next_cursor;
            if cursor == 0 {
                break;
            }
        }
        Ok(sessions)
    }
}
