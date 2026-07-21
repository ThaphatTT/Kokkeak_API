#![deny(unsafe_code)]

pub mod admin_order_service;
pub mod admin_user;
pub mod audit;
pub mod auth;
pub mod catalog;
pub mod category_job_main;
pub mod category_job_service_main;
pub mod category_job_service_sub;
pub mod category_job_service_sub_fee;
pub mod category_job_service_sub_warranty;
pub mod chat;
pub mod master;
pub mod order;
pub mod payment;
pub mod permission;
pub mod rate_limit;
pub mod user;
pub mod user_role;

pub use audit::{AuditEvent, AuditLogger, NoopAuditLogger, TestAuditLogger};
pub use auth::{AuthOutcome, AuthService, LoginInput, RegisterInput};
pub use catalog::{CatalogService, ServiceListPage};
pub use category_job_main::CategoryJobMainService;
pub use category_job_service_main::CategoryJobServiceMainService;
pub use category_job_service_sub::CategoryJobServiceSubService;
pub use category_job_service_sub_fee::CategoryJobServiceSubFeeService;
pub use category_job_service_sub_warranty::CategoryJobServiceSubWarrantyService;
pub use chat::{BroadcastTransport, ChatEvent, ChatService, ChatTransport, ChatUseCaseError};
pub use master::MasterDropdownService;
pub use order::{OrderListPage, OrderService};
pub use payment::{ConfirmPaymentInput, ConfirmPaymentResult, CreatePaymentInput, PaymentService};
pub use permission::{PermissionUserListPage, PermissionUserService};
pub use rate_limit::{
    AllowAllLoginRateLimiter, LoginRateLimiter, NoopLoginRateLimiter, RateLimitDecision,
};
pub use user::UserService;
pub use user_role::UserRoleService;
