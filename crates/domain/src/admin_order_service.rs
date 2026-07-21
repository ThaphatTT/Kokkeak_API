use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourcingMode {
    Open = 1,
    Managed = 2,
    Invited = 3,
}

impl SourcingMode {
    pub fn as_i32(&self) -> i32 {
        match self {
            Self::Open => 1,
            Self::Managed => 2,
            Self::Invited => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BidderType {
    Company = 1,
    User = 2,
    Team = 3,
}

impl BidderType {
    pub fn as_i32(&self) -> i32 {
        match self {
            Self::Company => 1,
            Self::User => 2,
            Self::Team => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyType {
    Single = 1,
    Package = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low = 1,
    Medium = 2,
    High = 3,
    Urgent = 4,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminCreateOrderResult {
    pub success: bool,
    pub message: String,
    pub data: Option<AdminCreateOrderData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminCreateOrderData {
    pub order_service_header_guid: String,
    pub order_no: String,
    pub workflow_status: i32,
    pub workflow_status_text: String,
    pub participant_count: i32,
    pub address_count: i32,
    pub body_count: i32,
    pub invitation_count: i32,
    pub address_mapping: Vec<AddressMapping>,
    pub body_mapping: Vec<BodyMapping>,
    pub invitation_mapping: Vec<InvitationMapping>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressMapping {
    pub client_key: String,
    pub order_service_address_guid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BodyMapping {
    pub client_key: String,
    pub order_service_body_guid: String,
    pub workflow_status: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationMapping {
    pub candidate_client_key: String,
    pub body_client_key: String,
    pub order_service_proposal_invitation_guid: String,
    pub bidder_key: String,
    pub bidder_type: i32,
    pub invitation_status: i32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AdminOrderListInput {
    pub keyword: Option<String>,
    pub workflow_status: Option<i32>,
    pub page: u32,
    pub page_size: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AdminOrderRow {
    pub order_service_header_guid: String,
    pub order_no: String,
    pub owner_user_guid: String,
    pub owner_name: String,
    pub sourcing_mode: i32,
    pub workflow_status: i32,
    pub workflow_status_text: String,
    pub currency: String,
    pub body_count: i32,
    pub total_amount: String,
    pub create_at: Option<String>,
    pub create_by: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AdminOrderPage {
    pub items: Vec<AdminOrderRow>,
    pub total_count: i64,
    pub page: u32,
    pub page_size: u32,
    pub total_page: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AdminOrderDetailRow {
    pub order_service_header_guid: String,
    pub order_no: String,
    pub owner_user_guid: String,
    pub owner_name: String,
    pub sourcing_mode: i32,
    pub approval_policy: i32,
    pub workflow_status: i32,
    pub workflow_status_text: String,
    pub currency: String,
    pub preferred_payment_method: String,
    pub note: String,
    pub body_count: i32,
    pub participant_count: i32,
    pub address_count: i32,
    pub invitation_count: i32,
    pub total_amount: String,
    pub create_at: Option<String>,
    pub create_by: String,
    pub update_at: Option<String>,
    pub update_by: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AdminOrderUpdateInput {
    pub order_service_header_guid: String,
    pub workflow_status: Option<i32>,
    pub note: Option<String>,
    pub update_by: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AdminOrderUpdateResult {
    pub success: bool,
    pub code: String,
    pub message: String,
    pub order_service_header_guid: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AdminOrderDeleteResult {
    pub success: bool,
    pub code: String,
    pub message: String,
    pub order_service_header_guid: String,
}
