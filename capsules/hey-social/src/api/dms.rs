//! Direct messages — now the shared `hey-core` DM engine.
//!
//! hey-social previously carried its own ~1337-line single-shot DM module. The
//! engine's `api::dms` is a strict superset: the same v1/v2 sealed-sender wire
//! and the same on-disk layout (`dm/contacts.json`, `dm/by-did/<did>.json`,
//! `dm/expiry.json`, `dm/peer-keys.json`) — so existing hey-social contacts and
//! history load unchanged (all new `DmContact`/`DmMessage` fields are
//! `#[serde(default)]`) — PLUS the M6 Double Ratchet (forward secrecy + PCS),
//! Anonymous identity mode, and M7 E2E attachments. Adopting it makes hey-social
//! and hey-chat ONE DM network.
//!
//! We re-export the whole engine module and only shadow the two functions whose
//! signatures gained an `IdentityMode` parameter, defaulting hey-social to the
//! stable federated identity (`Regular`) — its existing behavior. Anonymous mode
//! is available to wire into the social UI later via the engine functions.

pub use hey_core::api::dms::*;

use hey_core::api::dms::IdentityMode;

/// Mint an invite for a new contact (hey-social presents its stable federated
/// identity). Thin compat shim over the engine's `generate_invite(label, mode)`.
pub async fn generate_invite(display_label: &str) -> Result<String, String> {
    hey_core::api::dms::generate_invite(display_label, IdentityMode::Regular).await
}

/// Accept someone's invite link (Regular identity). Compat shim over the
/// engine's `accept_invite(token, mode)`.
pub async fn accept_invite(token: &str) -> Result<String, String> {
    hey_core::api::dms::accept_invite(token, IdentityMode::Regular).await
}
