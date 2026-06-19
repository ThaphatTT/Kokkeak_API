//! Application layer
//!
//! Use cases: each public function orchestrates one business action
//! (e.g. `create_order`, `login`, `approve_technician`).
//!
//! Depends on `domain` for entities/traits and on `infra` only
//! through `Arc<dyn Trait>` (constructor-injected).

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod auth;
pub mod catalog;
pub mod chat;
pub mod order;
pub mod payment;
pub mod user;

pub use auth::{AuthOutcome, AuthService, LoginInput, RegisterInput};
pub use catalog::{CatalogService, ServiceListPage};
pub use chat::{BroadcastTransport, ChatEvent, ChatService, ChatTransport, ChatUseCaseError};
pub use order::{OrderListPage, OrderService};
pub use payment::{ConfirmPaymentInput, ConfirmPaymentResult, CreatePaymentInput, PaymentService};
pub use user::UserService;
