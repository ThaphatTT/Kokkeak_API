//! Service catalog domain (แคตตาล็อกบริการ — M3).
//!
//! A `ServiceCategory` represents a bookable service (e.g. "แอร์เย็นไม่ทำงาน",
//! "ประปารั่ว"). Categories are master data — cached aggressively
//! (group A, TTL 1-24h per AGENTS.md § 9.3).

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One bookable service in the catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ServiceCategory {
    /// Stable identifier.
    pub id: Uuid,
    /// URL-safe code, unique per service (e.g. `"ac-not-cooling"`).
    /// Used as a stable handle in URLs and i18n message keys.
    pub code: String,
    /// Default service price (LAK). Optional — some categories are
    /// quoted after on-site inspection.
    pub default_price: Option<Decimal>,
    /// Default warranty period in days.
    pub warranty_days: i32,
    /// Whether this category is shown to customers right now.
    pub active: bool,
    /// Sort order (ascending) for admin / customer list display.
    pub sort_order: i32,
    /// Created timestamp.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn service_category_json_round_trip() {
        let s = ServiceCategory {
            id: Uuid::new_v4(),
            code: "ac-not-cooling".into(),
            default_price: Some(Decimal::from_str("250000.00").unwrap()),
            warranty_days: 30,
            active: true,
            sort_order: 10,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let v: serde_json::Value = serde_json::to_value(&s).unwrap();
        assert_eq!(v["code"], "ac-not-cooling");
        assert_eq!(v["warranty_days"], 30);
        assert_eq!(v["active"], true);
    }
}
