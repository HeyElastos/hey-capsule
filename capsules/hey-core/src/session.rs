// Minimal session state — Rust port of capsules/hey-social/client/src/lib/session.js
// (slimmed to what's needed for sign-in gating).
//
// Stores { auth_key_hex, did_key, name } in localStorage so a page reload
// preserves the signed-in identity. Source of truth for "am I signed in?"
// is whether `current()` returns Some.

use gloo_storage::{LocalStorage, Storage as _};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub auth_key_hex: String,
    pub did_key: String,
    pub name: String,
    /// ML-KEM-768 secret key, base64-encoded. Generated once at first
    /// sign-in; the matching public key gets published to peers via every
    /// dm.message envelope so they can encrypt to us. ~2400 bytes b64.
    #[serde(default)]
    pub ml_kem_secret_b64: String,
    /// ML-KEM-768 public key, base64. ~1580 b64 chars.
    #[serde(default)]
    pub ml_kem_public_b64: String,
}

pub fn current() -> Option<Session> {
    LocalStorage::get::<Session>(crate::ctx::session_key()).ok()
}

pub fn set(session: &Session) {
    let _ = LocalStorage::set(crate::ctx::session_key(), session);
}

pub fn clear() {
    let _ = LocalStorage::delete(crate::ctx::session_key());
}

/// Full identity wipe — for "I'm done with this device" / shared-machine
/// workflows. Drops the Session record (Ed25519 seed + ML-KEM secret),
/// the welcomed flag, and sessionStorage. Storage under `Hey/` (dm
/// contacts, conversation logs, outbox, peer-keys cache) is NOT
/// cleared here — the caller invokes `api::dms::wipe_dm_storage()`
/// next so a partial wipe failure can't leave dangling state.
pub fn wipe_identity() {
    let _ = LocalStorage::delete(crate::ctx::session_key());
    let _ = LocalStorage::delete(crate::ctx::welcomed_key());
    if let Some(win) = web_sys::window() {
        if let Ok(Some(s)) = win.session_storage() {
            let _ = s.clear();
        }
    }
}

pub fn welcomed() -> bool {
    LocalStorage::get::<bool>(crate::ctx::welcomed_key()).unwrap_or(false)
}

pub fn mark_welcomed() {
    let _ = LocalStorage::set(crate::ctx::welcomed_key(), true);
}
