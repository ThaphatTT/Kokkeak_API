//! Application layer
//!
//! Use cases: each public function orchestrates one business action
//! (e.g. `create_order`, `login`, `approve_technician`).
//!
//! Depends on `domain` for entities/traits and on `infra` only
//! through `Arc<dyn Trait>` (constructor-injected).

#![deny(unsafe_code)]
#![warn(missing_docs)]
