//! Repository ports (พอร์ต repository — Hexagonal pattern).
//!
//! Every persistence concern lives behind an `async_trait`. Application
//! code depends on these traits; the concrete adapter (SQL Server,
//! MongoDB, JSON-DB simulation) is wired in `api::main`. The trait
//! surface is **deliberately small** — repository methods mirror
//! use-case intent, not SQL/Mongo jargon.
//!
//! Per AGENTS.md § 6, this module belongs to `domain` so the domain
//! layer can express its persistence expectations without depending
//! on any framework / driver.

pub mod catalog;
pub mod chat;
pub mod master;
pub mod order;
pub mod payment;
pub mod permission;
pub mod translation;
pub mod user;
pub mod user_role;

pub use catalog::ServiceRepository;
pub use chat::{ChatMembership, ChatRepoError, ChatRepository, MessagePage};
pub use master::MasterDropdownRepository;
pub use order::OrderRepository;
pub use payment::{PaymentRepoError, PaymentRepository};
pub use permission::PermissionUserRepository;
pub use translation::{TranslationError, TranslationRepository};
pub use user::UserRepository;
pub use user_role::UserRoleRepository;
