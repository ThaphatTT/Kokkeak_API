

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ServiceCategory {

    pub id: Uuid,

    pub code: String,

    pub default_price: Option<Decimal>,

    pub warranty_days: i32,

    pub active: bool,

    pub sort_order: i32,

    pub created_at: DateTime<Utc>,

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
