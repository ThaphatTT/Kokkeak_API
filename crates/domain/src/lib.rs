//! Domain layer
//!
//! Pure Rust: entities, value objects, business rules, and repository
//! **traits** (ports).
//!
//! **Dependency rule** (AGENTS.md § 6): this crate MUST NOT import
//! anything from the framework or DB world (no `axum`, no `tiberius`,
//! no `mongodb`). All IO is expressed through traits in this crate.

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod auth;
pub mod cache;
pub mod catalog;
pub mod chat;
pub mod error;
pub mod health;
pub mod idempotency;
pub mod master;
pub mod matching;
pub mod order;
pub mod pagination;
pub mod payment;
pub mod permission;
pub mod queue;
pub mod storage;
pub mod traits;
pub mod user;

pub use auth::{AuthError, AuthSession, Claims, PublicUser, TokenKind, TokenPair};
pub use cache::{Cache, CacheError, CacheExt, CacheGroup, CacheKey, InvalidationStream};
pub use catalog::ServiceCategory;
pub use chat::{ChatError, ChatMessage, ChatRoom, MessageId, Participant, RoomId, RoomSummary};
pub use error::LocalizedError;
pub use health::{CheckOutcome, HealthCheck, HealthError, HealthRegistry, ReadyReport};
pub use idempotency::{CachedResponse, IdempotencyStore};
pub use master::MasterDropdownRow;
pub use order::{Order, OrderStatus};
pub use pagination::{Cursor, CursorError};
pub use payment::{
    commission, Commission, Payment, PaymentError, PaymentStatus, Payout, PayoutStatus,
};
pub use permission::{
    PermissionOverrideUpdateItem, PermissionOverrideUpdateResult, PermissionUpdateRow,
    PermissionUserDetailRow, PermissionUserGroup, PermissionUserGroupEntry, PermissionUserListRow,
    UserRolePermission, UserRolePermissionRow, UserRoleWithPermissions,
};
pub use queue::{QueueError, QueueMessage, QueuePort};
pub use storage::{PutResult, Storage, StorageError, StorageKey};
pub use traits::chat::{ChatMembership, ChatRepoError, ChatRepository, MessagePage};
pub use traits::master::MasterDropdownRepository;
pub use traits::order::OrderRepository;
pub use traits::payment::{PaymentRepoError, PaymentRepository};
pub use traits::permission::PermissionUserRepository;
pub use traits::translation::{TranslationError, TranslationRepository};
pub use traits::user::RepoError;
pub use traits::user_role::UserRoleRepository;
pub use traits::{ServiceRepository, UserRepository};
pub use user::{Permission, Role, User, UserListRow, UserStatus};
