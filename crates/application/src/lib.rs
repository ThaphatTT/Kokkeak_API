

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod admin_user;
pub mod audit;
pub mod auth;
pub mod catalog;
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
