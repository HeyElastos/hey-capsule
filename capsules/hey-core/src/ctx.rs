//! Per-capsule identity, injected by the consuming bin crate at boot.
//!
//! `hey-core` is shared by hey-social and hey-messenger. The transport,
//! storage, and session layers need per-capsule values: the capsule id in
//! `/api/apps/<id>/*` URLs, the private storage namespace, the localStorage/
//! sessionStorage keys, and the boot-time capability wants-list. Rather than
//! bake hey-social's values into the shared crate, the bin crate calls
//! `hey_core::ctx::init(CapsuleCtx { .. })` once in `main()` before mounting,
//! and the engine reads them through the accessors below.
//!
//! hey-social supplies: capsule_id "hey-social", namespace "Hey",
//! session_key "hey-social-session", and its 5-entry boot wants-list.
//! hey-messenger supplies: capsule_id "hey-messenger", namespace
//! "HeyMessenger", session_key "hey-messenger-session" (separate per-app
//! session — same DID, independent sign-in), and a peer/blobs/did wants-list.

use std::cell::OnceCell;

#[derive(Clone, Copy)]
pub struct CapsuleCtx {
    /// Capsule id used in `/api/apps/<id>/*` (e.g. "hey-social", "hey-messenger").
    pub capsule_id: &'static str,
    /// Private per-capsule storage namespace under the user root (e.g. "Hey").
    pub private_namespace: &'static str,
    /// localStorage key for the persisted `Session` record.
    pub session_key: &'static str,
    /// localStorage key for the per-device "welcomed" flag.
    pub welcomed_key: &'static str,
    /// sessionStorage key: "launch token redeemed in this tab" sticky bit.
    pub session_redeemed_key: &'static str,
    /// sessionStorage key caching the Home launch token for this tab.
    pub home_launch_token_key: &'static str,
    /// sessionStorage key for the legacy `/runtime-token` bearer.
    pub runtime_token_key: &'static str,
    /// sessionStorage key for the capability-token cache.
    pub token_store_key: &'static str,
    /// sessionStorage key memoizing the storage route mode (patch-0002/legacy).
    pub route_mode_key: &'static str,
    /// Boot-time capability wants-list: (resource, action) pairs requested up
    /// front. Must stay within what the capsule.json `permissions` grant.
    pub boot_capabilities: &'static [(&'static str, &'static str)],
}

thread_local! {
    static CTX: OnceCell<CapsuleCtx> = const { OnceCell::new() };
}

/// Install the per-capsule context. Call once in the bin crate's `main()`
/// before any runtime/session/chat call. A second call is ignored.
pub fn init(ctx: CapsuleCtx) {
    CTX.with(|c| {
        let _ = c.set(ctx);
    });
}

fn get() -> CapsuleCtx {
    CTX.with(|c| {
        *c.get().expect(
            "hey_core::ctx::init(CapsuleCtx { .. }) must be called in main() before using the engine",
        )
    })
}

pub fn capsule_id() -> &'static str {
    get().capsule_id
}
pub fn private_namespace() -> &'static str {
    get().private_namespace
}
pub fn session_key() -> &'static str {
    get().session_key
}
pub fn welcomed_key() -> &'static str {
    get().welcomed_key
}
pub fn session_redeemed_key() -> &'static str {
    get().session_redeemed_key
}
pub fn home_launch_token_key() -> &'static str {
    get().home_launch_token_key
}
pub fn runtime_token_key() -> &'static str {
    get().runtime_token_key
}
pub fn token_store_key() -> &'static str {
    get().token_store_key
}
pub fn route_mode_key() -> &'static str {
    get().route_mode_key
}
pub fn boot_capabilities() -> &'static [(&'static str, &'static str)] {
    get().boot_capabilities
}
