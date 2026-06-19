//! Pub/sub adapters for cross-instance fan-out (M8).
//!
//! - [`redis_chat::RedisChatPubSub`] — wraps the in-process
//!   [`kokkak_application::BroadcastTransport`] and rebroadcasts
//!   on Redis pub/sub channels (`chat:room:{id}`) so every API
//!   instance can fan the same event out to its WebSocket
//!   subscribers.

pub mod redis_chat;

pub use redis_chat::RedisChatPubSub;
