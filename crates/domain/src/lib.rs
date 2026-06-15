//! Domain layer
//!
//! Pure Rust: entities, value objects, business rules, and repository
//! **traits** (ports).
//!
//! **Dependency rule** (AGENTS.md § 6): this crate MUST NOT import
//! anything from the framework or DB world (no `axum`, no `tiberius`,
//! no `mongodb`). All IO is expressed through traits in `domain::traits`.

#![deny(unsafe_code)]
#![warn(missing_docs)]
