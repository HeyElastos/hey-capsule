// Minimal session state — Rust port of capsules/hey-social/client/src/lib/session.js
// (slimmed to what's needed for sign-in gating).
//
// Stores { auth_key_hex, did_key, name } in localStorage so a page reload
// preserves the signed-in identity. Source of truth for "am I signed in?"
// is whether `current()` returns Some.

use gloo_storage::{LocalStorage, Storage as _};
use serde::{Deserialize, Serialize};

const SESSION_KEY: &str = "hey-social-rust-session";

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
    LocalStorage::get::<Session>(SESSION_KEY).ok()
}

pub fn set(session: &Session) {
    let _ = LocalStorage::set(SESSION_KEY, session);
}

pub fn clear() {
    let _ = LocalStorage::delete(SESSION_KEY);
}
