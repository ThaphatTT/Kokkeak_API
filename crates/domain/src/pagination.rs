//! Pagination helpers (ตัวช่วยแบ่งหน้า).
//!
//! AGENTS.md § 11.4 mandates keyset/cursor pagination (no offset —
//! it goes quadratic on deep pages). This module defines the cursor
//! type and a tiny codec; the application / API layers interpret
//! the opaque string.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Opaque cursor (base64url-encoded JSON).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor(String);

impl Cursor {
    /// Encode a value into a cursor string.
    pub fn encode<T: Serialize>(value: &T) -> Result<Self, CursorError> {
        let json = serde_json::to_vec(value)?;
        Ok(Self(URL_SAFE_NO_PAD.encode(json)))
    }

    /// Decode a cursor back into a typed value.
    pub fn decode<T: for<'de> Deserialize<'de>>(&self) -> Result<T, CursorError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(self.0.as_bytes())
            .map_err(|e| CursorError::Codec(e.to_string()))?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Borrow the raw string form.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Build from a raw string (e.g. from a query parameter).
    pub fn from_raw(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::fmt::Display for Cursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::str::FromStr for Cursor {
    type Err = CursorError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(CursorError::Empty);
        }
        Ok(Self(s.to_string()))
    }
}

/// Typed cursor errors.
#[derive(Debug, Error)]
pub enum CursorError {
    /// Empty string.
    #[error("empty cursor")]
    Empty,
    /// Base64 / JSON decode failure.
    #[error("cursor codec error: {0}")]
    Codec(String),
    /// JSON (de)serialization failure.
    #[error("cursor json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use uuid::Uuid;

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Page {
        after_id: Uuid,
    }

    #[test]
    fn cursor_round_trip_preserves_value() {
        let p = Page {
            after_id: Uuid::new_v4(),
        };
        let c = Cursor::encode(&p).unwrap();
        let decoded: Page = c.decode().unwrap();
        assert_eq!(decoded, p);
    }

    #[test]
    fn cursor_display_matches_raw() {
        let c = Cursor::from_raw("abc-123");
        assert_eq!(c.to_string(), "abc-123");
        assert_eq!(c.as_str(), "abc-123");
    }

    #[test]
    fn from_str_rejects_empty() {
        assert!(matches!(
            "".parse::<Cursor>().unwrap_err(),
            CursorError::Empty
        ));
    }
}
