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
// KEPT local (social-specific or social-ahead — not yet promoted into the
// engine):
//   * `content` / `ipfs` — the content-provider wrapper, which carries
//     hey-social's immutable-CID byte cache (social-ahead; Phase B promotes it).
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
    acquire_boot_capabilities, api_base, api_url, bearer_ready, did_provider,
    ensure_capability_token, home_launch_token, inherit_session, peer, provider_call,
    redeem_launch_token, scrub_launch_token_from_url, session_current, shared_read_json,
    shared_write_json, storage, transcoder, upstream_fetch, RuntimeError, SharedIdentity,
};

fn window() -> web_sys::Window {
    web_sys::window().expect("no window")
}

fn encode_uri(s: &str) -> String {
    js_sys::encode_uri_component(s).as_string().unwrap_or_default()
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

// ── Content provider (publish / fetch / ensure / unpublish) ──────────
//
// Upstream Elastos Runtime expects app capsules to call the abstract
// content provider (elastos://content/*), NOT the raw ipfs provider —
// per CONTENT_AVAILABILITY.md the ipfs provider is system-only. The
// content provider sits between us and Kubo/supernode/etc, handling
// the publish flow end-to-end (local pin → network-available with a
// signed availability receipt) and gating encrypted blobs through dDRM.
//
// KEPT local because it carries hey-social's immutable-CID byte cache
// (social-ahead of the engine's content module — Phase B promotes the cache
// up, after which this can re-export the engine like the rest). The wrappers
// call the RE-EXPORTED provider_call / api_base, and the kept `encode_uri`.

pub mod content {
    use super::{api_base, encode_uri, provider_call, RuntimeError, B64};
    use base64::Engine;
    use serde_json::{json, Value};
    use std::cell::RefCell;
    use std::collections::{HashMap, VecDeque};

    // ── Immutable-CID byte cache ─────────────────────────────────────────
    // Content addressing makes this trivially safe: a CID is the hash of its
    // bytes, so cached bytes can never go stale. The cache is bounded by both
    // entry count and total bytes (FIFO eviction) so a long session can't
    // grow the heap without limit, and we skip blobs above a per-entry cap —
    // large media is rendered via the gateway <img>/<video> path, not
    // get_bytes, so in practice the cache stays full of small dag-cbor post
    // bodies. A persistent tier (Cache API / IndexedDB) can layer underneath
    // later; it would share `cache_key`.
    const CID_CACHE_MAX_ENTRIES: usize = 512;
    const CID_CACHE_MAX_BYTES: usize = 8 * 1024 * 1024;
    const CID_CACHE_MAX_ENTRY_BYTES: usize = 1024 * 1024;

    thread_local! {
        static CID_CACHE: RefCell<CidCache> = RefCell::new(CidCache::default());
    }

    #[derive(Default)]
    struct CidCache {
        map: HashMap<String, Vec<u8>>,
        order: VecDeque<String>,
        total_bytes: usize,
    }

    impl CidCache {
        fn get(&self, key: &str) -> Option<Vec<u8>> {
            self.map.get(key).cloned()
        }

        fn put(&mut self, key: String, bytes: &[u8]) {
            // Skip oversized blobs and re-inserts (CIDs are immutable, so a
            // cached entry is already the canonical bytes).
            if bytes.len() > CID_CACHE_MAX_ENTRY_BYTES || self.map.contains_key(&key) {
                return;
            }
            while !self.order.is_empty()
                && (self.order.len() >= CID_CACHE_MAX_ENTRIES
                    || self.total_bytes + bytes.len() > CID_CACHE_MAX_BYTES)
            {
                if let Some(old) = self.order.pop_front() {
                    if let Some(v) = self.map.remove(&old) {
                        self.total_bytes -= v.len();
                    }
                }
            }
            self.total_bytes += bytes.len();
            self.order.push_back(key.clone());
            self.map.insert(key, bytes.to_vec());
        }
    }

    fn cache_key(cid: &str, path: Option<&str>) -> String {
        match path {
            // \u{1f} (unit separator) can't appear in a CID or a path segment,
            // so it's a collision-free join.
            Some(p) => format!("{cid}\u{1f}{p}"),
            None => cid.to_string(),
        }
    }

    pub async fn add_bytes(
        bytes: &[u8],
        filename: &str,
        pin: bool,
    ) -> Result<Value, RuntimeError> {
        // `pin=true` historically meant "we want this kept around"; mirror
        // that as the network_default availability policy. `pin=false`
        // maps to local_pin so the bytes are still recoverable on this
        // node but no replication is requested.
        let policy = if pin { "network_default" } else { "local_pin" };
        let body = json!({
            "data": B64.encode(bytes),
            "filename": filename,
            "policy": policy,
        });
        provider_call("content", "publish", body).await
    }

    pub async fn get_bytes(cid: &str, path: Option<&str>) -> Result<Vec<u8>, RuntimeError> {
        // A (cid, path) pair maps to bytes that can never change, so a cache
        // hit is always correct — this collapses repeat fetches (feed
        // re-render, scroll-back, navigating away and back) to one round-trip.
        let key = cache_key(cid, path);
        if let Some(hit) = CID_CACHE.with(|c| c.borrow().get(&key)) {
            return Ok(hit);
        }
        let mut body = json!({ "cid": cid });
        if let Some(p) = path {
            body["path"] = Value::String(p.into());
        }
        let resp = provider_call("content", "fetch", body).await?;
        let b64 = resp
            .get("data")
            .and_then(|d| d.get("data"))
            .and_then(|d| d.as_str())
            .or_else(|| resp.get("data").and_then(|d| d.as_str()))
            .ok_or_else(|| {
                RuntimeError::new(format!("content.fetch({cid}): no data in response"))
            })?;
        let bytes = B64
            .decode(b64)
            .map_err(|e| RuntimeError::new(format!("content.fetch base64: {e}")))?;
        CID_CACHE.with(|c| c.borrow_mut().put(key, &bytes));
        Ok(bytes)
    }

    // The IPFS gateway is proxied by nginx at /<API_BASE>/ipfs/<CID>; CIDs are
    // content-addressed so possession of the CID is itself the access token,
    // making this safe for direct <img> src binding (which can't carry headers).
    // (The gateway is an HTTP byte server, not the restricted provider RPC —
    // capsules are still allowed to fetch through it.)
    pub fn gateway_url(cid: &str, path: Option<&str>) -> String {
        let suffix = match path {
            Some(p) => format!("/{}", p.trim_start_matches('/')),
            None => String::new(),
        };
        format!("{}/ipfs/{}{}", api_base(), encode_uri(cid), suffix)
    }

    pub async fn pin(cid: &str) -> Result<Value, RuntimeError> {
        provider_call(
            "content",
            "ensure",
            json!({ "cid": cid, "policy": "network_default" }),
        )
        .await
    }
    pub async fn unpin(cid: &str) -> Result<Value, RuntimeError> {
        provider_call("content", "unpublish", json!({ "cid": cid })).await
    }

    // Extract a CID from a publish response. Handles both shapes:
    //   * legacy ipfs-provider:           { cid } or { data: { cid } }
    //   * upstream availability receipt:  { payload: { cid, uri, ... }, signer_did, signature }
    // Callers use this so they don't have to know which provider
    // implementation is on the other end.
    pub fn extract_cid(resp: &Value) -> Option<String> {
        resp.get("payload")
            .and_then(|p| p.get("cid"))
            .and_then(|c| c.as_str())
            .or_else(|| resp.get("data").and_then(|d| d.get("cid")).and_then(|c| c.as_str()))
            .or_else(|| resp.get("cid").and_then(|c| c.as_str()))
            .map(String::from)
    }
}

// Compatibility alias — many call sites still write `runtime::ipfs::*`.
// They get the content-provider wiring transparently.
pub use content as ipfs;

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
