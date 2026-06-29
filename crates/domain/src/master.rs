//! Master-data DTOs (M20+).
//!
//! Shared reference data consumed by every client (mobile,
//! customer web, admin web). Lives in `domain` per AGENTS.md §6.
//!
//! ## Why a generic shape (`value` / `label`) instead of a typed
//! struct per master table
//!
//! The same wire contract is reused across every dropdown: clients
//! pattern-match on `value` (the stable GUID string) and surface
//! `label` in the UI. Adding `MasterCountry { code, dial_code, ... }`
//! later for a richer admin column view doesn't break the dropdown
//! endpoint — a future `CountryDetail` DTO sits next to this one.
//!
//! ponytail: deliberately thin (`String` x2). No enum mapping, no
//! language fallback — the SP returns whatever name it stores
//! (Thai by default; an i18n layer lands in M20+ when a second
//! locale is in `master_country_*_en`). Ceiling: when a master
//! type needs a wider payload (e.g. provinces with `region_code`),
//! add a sibling struct rather than overloading this one.

use serde::{Deserialize, Serialize};

/// One row of a master-data dropdown (label / value pair).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MasterDropdownRow {
    /// Stable identifier — the master table's GUID string. Front-ends
    /// submit this when creating an order / profile so the binding
    /// survives a label rename.
    pub value: String,
    /// Human-readable label shown in the dropdown UI (single locale;
    /// see the module docs for the i18n ceiling).
    pub label: String,
}
