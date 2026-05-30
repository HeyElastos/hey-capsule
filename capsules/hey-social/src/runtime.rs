// Runtime adapter — now a thin shim over the shared hey-core engine.
//
// The transport/auth/storage CORE — provider_call, capability tokens, launch-
// token redemption (/session/start + /runtime-token), storage dispatch, the
// peer/transcoder/did wrappers, session inherit/introspection — is RE-EXPORTED
// from hey_core::runtime. One implementation, shared with hey-messenger,
// parameterized per-capsule by the CapsuleCtx wired in main.rs. Each engine
// function was verified equivalent to hey-social's former local copy modulo
// that ctx (see docs/hey-core-migration.md).
//
// `content` / `ipfs` is now re-exported too: the immutable-CID byte cache was
// promoted INTO hey_core::runtime::content (Phase B), so both apps share one
// cached implementation.
//
// KEPT local (social-specific or social-ahead — not yet promoted into the
// engine):
//   * `identity_provider` — the runtime-held-key wrapper events.rs/api::dms
//     depend on; kept local until its size-drift vs the engine is reconciled.
//   * boot-splash helpers (boot_log / warp_boot_into_feed / hide_boot_splash /
//     sleep_ms) — pure social UI.

#![allow(dead_code)]

use base64::engine::general_purpose::STANDARD as B64;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

// ── Shared engine surface ─────────────────────────────────────────────
// Auth/login (home-token redemption, /session/start, capability tokens),
// transport (provider_call, peer, did, transcoder), storage (per-capsule +
// shared), and session inherit/introspection. Verified equivalent to
// hey-social's former local copies modulo CapsuleCtx.
pub use hey_core::runtime::{
    acquire_boot_capabilities, api_base, api_url, bearer_ready, content, did_provider,
    ensure_capability_token, home_launch_token, inherit_session, ipfs, peer, provider_call,
    redeem_launch_token, scrub_launch_token_from_url, session_current, shared_read_json,
    shared_write_json, storage, transcoder, upstream_fetch, RuntimeError, SharedIdentity,
};

fn window() -> web_sys::Window {
    web_sys::window().expect("no window")
}


// ── Identity (runtime-held signing key — the wallet model) ────────────
//
// The capsule asks the runtime to sign / ECDH / decapsulate so the user's
// key never lives in the browser. Mirrors hey-core::runtime::identity_provider
// and the identity-projection-provider wire (whoami/pubkeys/sign/x25519_dh/
// ml_kem_decapsulate/verify). A provider-backed session (did:key, empty local
// seed) routes signing + DM decryption through here; local-seed sessions are
// the fallback so removing the fork patch leaves a working app.
//
// KEPT local: events.rs + api::dms depend on this exact wrapper; the engine's
// identity_provider has drifted in size, so converge it deliberately later
// rather than swap blindly. The wrappers call the RE-EXPORTED provider_call.

pub mod identity_provider {
    use super::{provider_call, RuntimeError, B64};
    use base64::Engine;
    use serde_json::{json, Value};

    /// Shared identity namespace for ALL Hey capsules — one did:key per user
    /// everywhere. MUST equal hey-core's HEY_NAMESPACE. Do NOT use the capsule id.
    pub const HEY_NAMESPACE: &str = "hey";

    pub async fn whoami(namespace: &str) -> Result<Value, RuntimeError> {
        provider_call("identity", "whoami", json!({ "namespace": namespace })).await
    }

    pub async fn pubkeys(namespace: &str) -> Result<Value, RuntimeError> {
        provider_call("identity", "pubkeys", json!({ "namespace": namespace })).await
    }

    pub async fn sign(namespace: &str, payload: &[u8]) -> Result<Value, RuntimeError> {
        provider_call(
            "identity",
            "sign",
            json!({ "namespace": namespace, "payload_b64": B64.encode(payload) }),
        )
        .await
    }

    pub async fn x25519_dh(namespace: &str, eph_pub: &[u8]) -> Result<Value, RuntimeError> {
        provider_call(
            "identity",
            "x25519_dh",
            json!({ "namespace": namespace, "eph_pub_b64": B64.encode(eph_pub) }),
        )
        .await
    }

    pub async fn ml_kem_decapsulate(namespace: &str, ct: &[u8]) -> Result<Value, RuntimeError> {
        provider_call(
            "identity",
            "ml_kem_decapsulate",
            json!({ "namespace": namespace, "ct_b64": B64.encode(ct) }),
        )
        .await
    }

    pub async fn verify(
        did_key: &str,
        payload: &[u8],
        signature_hex: &str,
    ) -> Result<Value, RuntimeError> {
        provider_call(
            "identity",
            "verify",
            json!({
                "did_key": did_key,
                "payload_b64": B64.encode(payload),
                "signature_hex": signature_hex,
            }),
        )
        .await
    }

    /// Pull a 32-byte `shared_b64` out of an x25519_dh / ml_kem_decapsulate response.
    pub fn shared_from(resp: &Value) -> Result<Vec<u8>, RuntimeError> {
        let b64 = resp
            .get("data")
            .and_then(|d| d.get("shared_b64"))
            .or_else(|| resp.get("shared_b64"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| RuntimeError::new("identity provider: no shared_b64 in response"))?;
        B64.decode(b64)
            .map_err(|e| RuntimeError::new(format!("identity provider shared_b64: {e}")))
    }
}


// ── Boot splash (social-only UI) ──────────────────────────────────────

/// Boot-trace logging. The no-tap boot path is otherwise silent (errors are
/// swallowed by `.ok()?` so a working app keeps booting on vanilla upstream),
/// which makes a hard-refresh on the box impossible to diagnose from DevTools.
pub fn boot_log(s: &str) {
    web_sys::console::info_1(&JsValue::from_str(&format!("[hey-social boot] {s}")));
}

fn boot_splash_el() -> Option<web_sys::Element> {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("hey-boot"))
}

/// True once the boot splash has already been dismissed (warped or faded),
/// so a second caller (e.g. the safety-net timer, or a later feed mount)
/// doesn't clobber the first, nicer transition.
fn boot_splash_dismissed(el: &web_sys::Element) -> bool {
    let c = el.get_attribute("class").unwrap_or_default();
    c.contains("warp-transition") || c.contains("hey-boot-hide")
}

/// Fly the `#hey-boot` splash out as the feed warps in — the no-tap → feed
/// tunnel. Reuses the onboarding `.warp-out` keyframe (scale up + blur +
/// fade to 0); the feed's own `.warp-in` makes it the same continuous warp.
/// No-op if the splash is absent (older build) or already dismissed.
pub fn warp_boot_into_feed() {
    if let Some(el) = boot_splash_el() {
        if !boot_splash_dismissed(&el) {
            let _ = el.set_attribute("class", "warp-transition");
        }
    }
}

/// Dismiss the `#hey-boot` splash with a plain fade — used when there is no
/// no-tap identity and we're revealing the passkey sign-in CTA underneath,
/// and as a route-agnostic safety net. No-op if already dismissed.
pub fn hide_boot_splash() {
    if let Some(el) = boot_splash_el() {
        if !boot_splash_dismissed(&el) {
            let _ = el.set_attribute("class", "hey-boot-hide");
        }
    }
}

pub async fn sleep_ms(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let _ = window()
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
    });
    let _ = JsFuture::from(promise).await;
}
