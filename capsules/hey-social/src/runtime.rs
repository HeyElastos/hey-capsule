// Runtime HTTP client — Rust port of capsules/hey-social/client/src/lib/runtime.js.
//
// One adapter between Hey-the-Rust-app and the Elastos Runtime's HTTP surface.
// Everything else (events.rs, pages/*, components/*) should call only the
// helpers exported from this module — when upstream rev's, this is the only
// file to touch.
//
// What's wired up:
//   * api_url + api_base                    — install-base-aware URL helper
//   * home_launch_token + bearer_ready      — launch envelope → session bearer
//                                             (patch 0001 /runtime-token exchange)
//   * provider_call                         — POST /api/provider/<scheme>/<op>
//   * peer / ipfs / did_provider            — typed wrappers over provider_call
//   * capability tokens (request + cache)   — X-Capability-Token header source
//   * storage (per-capsule "Hey" namespace) — patch-0002 OR legacy /api/localhost/
//   * shared_storage                        — cross-capsule .AppData/* paths
//
// Not yet ported: transcoder + elacity + IPLD encode/decode (post.create.v2
// dag-cbor envelope) + non-extractable CryptoKey signing. The Rust app uses
// ed25519-compact in-process today; future hardening should mirror the
// React lib/keystore.js path.

#![allow(dead_code)]

use base64::engine::general_purpose::STANDARD as B64;
use gloo_storage::{SessionStorage, Storage as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestCredentials, RequestInit, Response};

pub const CAPSULE_ID: &str = "hey-social";
const PRIVATE_NAMESPACE: &str = "Hey";

/// Sticky bit: "we've successfully exchanged the launch token for an
/// app-scoped session cookie in this tab." Cleared when a fresh launch
/// token arrives in the URL (different from the cached one). Set after
/// /api/apps/<id>/session/start returns 200. The session itself rides
/// the HttpOnly cookie the runtime sets — we never see the credential.
const SESSION_REDEEMED_KEY: &str = "hey-session-redeemed";
const HOME_LAUNCH_TOKEN_KEY: &str = "hey-home-launch-token";
const ROUTE_MODE_KEY: &str = "hey-storage-route-mode";
const TOKEN_STORE_KEY: &str = "hey-capability-tokens";
/// Bearer token cache. The fork's `/runtime-token` endpoint returns
/// a session bearer in the response body but does NOT set a cookie —
/// so if we hit that fallback path we have to stash the bearer here
/// and add it as `Authorization: Bearer …` on every subsequent fetch.
/// The newer `/session/start` cookie path leaves this empty (and that's
/// fine — the cookie rides via `credentials: 'include'` automatically).
const RUNTIME_TOKEN_KEY: &str = "hey-runtime-token";

#[derive(Debug, Clone)]
pub struct RuntimeError {
    pub message: String,
    pub status: Option<u16>,
}

impl RuntimeError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status: None,
        }
    }
    pub fn with_status(message: impl Into<String>, status: u16) -> Self {
        Self {
            message: message.into(),
            status: Some(status),
        }
    }
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.status {
            Some(s) => write!(f, "{} (HTTP {})", self.message, s),
            None => write!(f, "{}", self.message),
        }
    }
}

fn window() -> web_sys::Window {
    web_sys::window().expect("no window")
}

pub fn api_base() -> String {
    let path = window().location().pathname().unwrap_or_default();
    if let Some(idx) = path.find("/apps/") {
        return path[..idx].to_string();
    }
    String::new()
}

pub fn api_url(path: &str) -> String {
    format!("{}{}", api_base(), path)
}

pub fn home_launch_token() -> Option<String> {
    let url_tok = read_url_token();
    if let Ok(Some(prev)) = SessionStorage::get::<Option<String>>(HOME_LAUNCH_TOKEN_KEY) {
        if let Some(fresh) = url_tok.as_ref() {
            if Some(fresh) != Some(&prev) {
                // Fresh launch from Home (e.g. user came back through
                // the launcher) — drop the redeemed bit and the
                // capability-token cache so we re-handshake against
                // the new launch token.
                let _ = SessionStorage::delete(SESSION_REDEEMED_KEY);
                let _ = SessionStorage::delete(TOKEN_STORE_KEY);
                let _ = SessionStorage::set(HOME_LAUNCH_TOKEN_KEY, fresh);
                return Some(fresh.clone());
            }
        }
        return Some(prev);
    }
    if let Some(fresh) = url_tok {
        let _ = SessionStorage::set(HOME_LAUNCH_TOKEN_KEY, &fresh);
        return Some(fresh);
    }
    None
}

fn read_url_token() -> Option<String> {
    let search = window().location().search().ok()?;
    let params = web_sys::UrlSearchParams::new_with_str(&search).ok()?;
    params
        .get("home_token")
        .or_else(|| params.get("runtime_token"))
}

/// Redeem the Home launch token for an app-scoped session.
///
/// Successful redemption: the runtime sets an app-scoped HttpOnly
/// cookie (`hey-session` or similar) on the response. Subsequent
/// fetches with `credentials: 'include'` carry it automatically; we
/// just remember "redeemed" in sessionStorage so we don't re-POST on
/// every fetch.
///
/// Endpoint reality (read from upstream @ 6d4c385 + the YNH fork):
///
///   * `POST /api/apps/<id>/session/start` is the canonical path.
///     Upstream v0.3 only routes it for the {documents, library,
///     system, wallet, browser} apps + chat-room — there is NO
///     generic per-app handler. The Hey YNH fork's patch 0001 adds
///     hey-social and hey-messenger to the allowlist; on that fork
///     `/session/start` works for us.
///
///   * `POST /api/apps/<id>/runtime-token` does NOT exist in
///     upstream and is not a generic upstream contract. We keep it as
///     a fallback only for older / patched YNH builds that happened
///     to expose it under that name.
///
/// We try `/session/start` first because the YNH-fork patch wires it
/// up; on stock upstream and on older YNH builds it 404s and we drop
/// to the legacy `/runtime-token` attempt. Either succeeding sets the
/// cookie; failure of both means no session for this load.
///
/// Returns true if a session is in place (already-redeemed OR fresh
/// redemption succeeded). Returns false if no launch token is
/// available or both endpoints rejected it.
///
/// Renamed from `bearer_ready` (the old name carried the bearer-token
/// model which we no longer use).
pub async fn redeem_launch_token() -> bool {
    if let Ok(Some(_)) = SessionStorage::get::<Option<String>>(SESSION_REDEEMED_KEY) {
        return true;
    }
    let Some(launch) = home_launch_token() else {
        return false;
    };
    let headers = json!({
        "Content-Type": "application/json",
        "x-elastos-home-token": launch,
    });

    // Try the canonical /session/start endpoint first.
    let canonical = api_url(&format!("/api/apps/{CAPSULE_ID}/session/start"));
    match fetch_raw(&canonical, "POST", Some("{}".to_string()), &headers).await {
        Ok(resp) if resp.ok() => {
            let _ = SessionStorage::set(SESSION_REDEEMED_KEY, "true");
            return true;
        }
        Ok(resp) if resp.status() != 404 && resp.status() != 405 => {
            log_warn(&format!(
                "[hey-social] session/start rejected: {}",
                resp.status()
            ));
            return false;
        }
        Ok(_) => {
            // 404 / 405 → older runtime, try legacy name.
        }
        Err(e) => {
            log_warn(&format!("[hey-social] session/start fetch error: {e:?}"));
            return false;
        }
    }

    let legacy = api_url(&format!("/api/apps/{CAPSULE_ID}/runtime-token"));
    match fetch_raw(&legacy, "POST", Some("{}".to_string()), &headers).await {
        Ok(resp) => {
            if !resp.ok() {
                log_warn(&format!(
                    "[hey-social] runtime-token (legacy) rejected: {}",
                    resp.status()
                ));
                return false;
            }
            // The legacy endpoint returns { "token": "<bearer>" } in JSON
            // and does NOT set an HttpOnly cookie. Stash the bearer so
            // upstream_fetch (and any other caller that goes through
            // fetch_raw) can attach it as Authorization: Bearer on
            // subsequent requests. Without this, every following call —
            // /api/session, /api/capability/request, /api/provider/* —
            // returns 401 because credentials: 'include' alone has
            // nothing to send.
            let parsed: Option<Value> = match JsFuture::from(resp.json().unwrap()).await {
                Ok(v) => serde_wasm_bindgen::from_value(v).ok(),
                Err(_) => None,
            };
            if let Some(tok) = parsed
                .as_ref()
                .and_then(|j| j.get("token"))
                .and_then(|t| t.as_str())
            {
                let _ = SessionStorage::set(RUNTIME_TOKEN_KEY, tok);
            } else {
                log_warn("[hey-social] runtime-token response had no `token` field");
                return false;
            }
            let _ = SessionStorage::set(SESSION_REDEEMED_KEY, "true");
            true
        }
        Err(_) => false,
    }
}

/// Read the cached session bearer (set by the legacy /runtime-token
/// redemption path). Returns None on the canonical /session/start
/// cookie path or when no redemption has happened yet.
fn current_runtime_token() -> Option<String> {
    SessionStorage::get::<String>(RUNTIME_TOKEN_KEY).ok()
}

/// Back-compat shim — the old name is still called from lib.rs and a
/// handful of internal sites. Forwards to `redeem_launch_token`. Will
/// be deleted once every caller is migrated; the public surface is
/// stable for now.
pub async fn bearer_ready() -> bool {
    redeem_launch_token().await
}

async fn fetch_raw(
    url: &str,
    method: &str,
    body: Option<String>,
    headers: &Value,
) -> Result<Response, JsValue> {
    let opts = RequestInit::new();
    opts.set_method(method);
    opts.set_credentials(RequestCredentials::Include);
    if let Some(b) = body.as_deref() {
        opts.set_body(&JsValue::from_str(b));
    }
    let req = Request::new_with_str_and_init(url, &opts)?;
    let hdrs = req.headers();
    if let Some(map) = headers.as_object() {
        for (k, v) in map {
            if let Some(s) = v.as_str() {
                hdrs.set(k, s)?;
            }
        }
    }
    // If we redeemed via the legacy /runtime-token path the runtime
    // handed us a session bearer in JSON instead of setting a cookie.
    // Attach it as Authorization: Bearer on every fetch (unless the
    // caller already set Authorization themselves). On the canonical
    // /session/start cookie path current_runtime_token() returns None
    // and this is a no-op — the cookie rides via credentials:'include'.
    if hdrs.get("Authorization").ok().flatten().is_none() {
        if let Some(tok) = current_runtime_token() {
            hdrs.set("Authorization", &format!("Bearer {tok}"))?;
        }
    }
    let resp_value = JsFuture::from(window().fetch_with_request(&req)).await?;
    resp_value.dyn_into::<Response>()
}

// Public helper used by passkey.rs to call upstream's /api/auth/passkey/*
// endpoints. Carries the app-session HttpOnly cookie via
// `credentials: 'include'` (set in fetch_raw). No more bearer-token
// injection — the cookie is the session credential.
pub async fn upstream_fetch(
    path: &str,
    method: &str,
    body: Option<String>,
) -> Result<Response, RuntimeError> {
    let _ = redeem_launch_token().await;
    let headers = json!({ "Content-Type": "application/json" });
    let url = api_url(path);
    fetch_raw(&url, method, body, &headers)
        .await
        .map_err(|e| RuntimeError::new(format!("fetch error: {e:?}")))
}

// ── Capability tokens ────────────────────────────────────────────────
//
// Tokens are bearer-style; we send them via X-Capability-Token. The
// runtime's auto-grant policy returns one immediately for any resource
// declared in capsule.json under permissions.{storage, messaging}. We
// cache by (resource, action) tuple in sessionStorage so subsequent
// reads skip the round-trip and survive intra-session navigation.
//
// Cache key encoding mirrors the JS code: `<action>::<resource>`.
//
// WASM is single-threaded so a thread_local RefCell suffices — no Mutex.

thread_local! {
    static TOKEN_CACHE: RefCell<HashMap<String, String>> = RefCell::new(load_token_store());
}

fn load_token_store() -> HashMap<String, String> {
    SessionStorage::get::<HashMap<String, String>>(TOKEN_STORE_KEY).unwrap_or_default()
}

fn save_token_store(cache: &HashMap<String, String>) {
    let _ = SessionStorage::set(TOKEN_STORE_KEY, cache);
}

fn cache_key(resource: &str, action: &str) -> String {
    format!("{action}::{resource}")
}

const FALLBACK_TOKEN: &str = "capsule-session";

fn token_for_resource(resource: &str, action: &str) -> String {
    TOKEN_CACHE.with(|c| {
        c.borrow()
            .get(&cache_key(resource, action))
            .cloned()
            .unwrap_or_else(|| FALLBACK_TOKEN.to_string())
    })
}

async fn request_capability_token(
    resource: &str,
    action: &str,
) -> Result<Option<String>, RuntimeError> {
    let _ = redeem_launch_token().await;
    let headers = json!({ "Content-Type": "application/json" });
    let body = json!({ "resource": resource, "action": action }).to_string();
    let resp = fetch_raw(
        &api_url("/api/capability/request"),
        "POST",
        Some(body),
        &headers,
    )
    .await
    .map_err(|e| RuntimeError::new(format!("capability/request fetch: {e:?}")))?;
    if !resp.ok() {
        return Err(RuntimeError::with_status(
            "capability/request",
            resp.status(),
        ));
    }
    let initial: Value = JsFuture::from(resp.json().unwrap())
        .await
        .map(|v| serde_wasm_bindgen::from_value(v).unwrap_or(Value::Null))
        .map_err(|e| RuntimeError::new(format!("capability/request json: {e:?}")))?;
    let status = initial
        .get("status")
        .and_then(|s| s.as_str())
        .unwrap_or_default();
    if status == "granted" {
        if let Some(tok) = initial.get("token").and_then(|t| t.as_str()) {
            return Ok(Some(tok.to_string()));
        }
        return Ok(None);
    }
    if status == "auto_denied" || status == "denied" {
        return Ok(None);
    }
    // Pending — poll. The shell renders a Grant prompt; user clicks Grant.
    // Backoff: 200/400/800/1500/2000ms; cap at 30 s overall.
    let request_id = initial
        .get("request_id")
        .and_then(|r| r.as_str())
        .ok_or_else(|| RuntimeError::new("capability/request: pending with no request_id"))?
        .to_string();
    let delays = [200, 400, 800, 1500, 2000];
    let start = js_sys::Date::now();
    let mut i = 0usize;
    while js_sys::Date::now() - start < 30_000.0 {
        let d = delays[i.min(delays.len() - 1)];
        sleep_ms(d).await;
        i += 1;
        let poll_headers = json!({});
        let url = api_url(&format!("/api/capability/request/{}", encode_uri(&request_id)));
        let Ok(r) = fetch_raw(&url, "GET", None, &poll_headers).await else {
            continue;
        };
        if !r.ok() {
            continue;
        }
        let Ok(json_v) = JsFuture::from(r.json().unwrap()).await else {
            continue;
        };
        let v: Value = serde_wasm_bindgen::from_value(json_v).unwrap_or(Value::Null);
        let s = v.get("status").and_then(|x| x.as_str()).unwrap_or_default();
        if s == "granted" {
            return Ok(v.get("token").and_then(|t| t.as_str()).map(String::from));
        }
        if s == "denied" || s == "expired" {
            return Ok(None);
        }
    }
    Ok(None)
}

pub async fn ensure_capability_token(resource: &str, action: &str) -> String {
    let key = cache_key(resource, action);
    if let Some(cached) = TOKEN_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return cached;
    }
    match request_capability_token(resource, action).await {
        Ok(Some(tok)) => {
            TOKEN_CACHE.with(|c| {
                let mut m = c.borrow_mut();
                m.insert(key, tok.clone());
                save_token_store(&m);
            });
            tok
        }
        _ => FALLBACK_TOKEN.to_string(),
    }
}

fn scheme_to_resource(scheme: &str) -> String {
    format!("elastos://{scheme}/*")
}

// ── Generic provider proxy ────────────────────────────────────────────

pub async fn provider_call(
    scheme: &str,
    op: &str,
    body: Value,
) -> Result<Value, RuntimeError> {
    let resource = scheme_to_resource(scheme);
    let cap = ensure_capability_token(&resource, "write").await;
    let mut headers = json!({
        "Content-Type": "application/json",
        "X-Capability-Token": cap,
    });
    // PATCH 0004 (transitional, removable) — capsule-side companion to the
    // fork's gateway-allowlist patch 0003. The gateway provider proxy
    // (gateway_provider_proxy.rs) authorizes /api/provider/* SOLELY on the
    // `x-elastos-home-token` header — the app-bound v2 launch envelope Home
    // minted — NOT on X-Capability-Token and NOT on the session cookie. So
    // without this header every provider call 403s "missing home launch
    // token", even once patch 0003 adds hey-social to the proxy allowlist.
    // Mirror the patch-0002 storage branch (build_storage_url), which already
    // attaches the cached launch token for the same reason.
    //
    // KILL CONDITION: drop this the moment the gateway proxy validates the
    // capability token (X-Capability-Token, sent above) per the dev's
    // capability-based model — "more providers, not more permissions" — so
    // provider auth rides the same credential as every other capability
    // check and the home-token header is redundant. Tracks fork patch 0003.
    if let Some(launch) = home_launch_token() {
        headers["x-elastos-home-token"] = Value::String(launch);
    }
    let url = format!(
        "{}/api/provider/{}/{}",
        api_base(),
        encode_uri(scheme),
        encode_uri(op)
    );
    let resp = fetch_raw(&url, "POST", Some(body.to_string()), &headers)
        .await
        .map_err(|e| RuntimeError::new(format!("provider_call fetch: {e:?}")))?;
    if !resp.ok() {
        return Err(RuntimeError::with_status(
            format!("provider_call({scheme}, {op})"),
            resp.status(),
        ));
    }
    let v = JsFuture::from(resp.json().unwrap())
        .await
        .map_err(|e| RuntimeError::new(format!("provider_call json: {e:?}")))?;
    Ok(serde_wasm_bindgen::from_value(v).unwrap_or(Value::Null))
}

// ── Peer (Carrier gossip) ─────────────────────────────────────────────

pub mod peer {
    use super::{provider_call, RuntimeError};
    use serde_json::{json, Value};

    pub async fn join_topic(topic: &str) -> Result<Value, RuntimeError> {
        provider_call("peer", "gossip_join", json!({ "topic": topic })).await
    }
    pub async fn leave_topic(topic: &str) -> Result<Value, RuntimeError> {
        provider_call("peer", "gossip_leave", json!({ "topic": topic })).await
    }

    #[derive(serde::Serialize)]
    pub struct PublishArgs<'a> {
        pub topic: &'a str,
        pub message: &'a str,
        pub sender_id: &'a str,
        pub ts: i64,
        pub signature: &'a str,
    }
    pub async fn publish(args: PublishArgs<'_>) -> Result<Value, RuntimeError> {
        let v = serde_json::to_value(args).map_err(|e| RuntimeError::new(format!("publish serialize: {e}")))?;
        provider_call("peer", "gossip_send", v).await
    }

    #[derive(serde::Serialize, Default)]
    pub struct RecvArgs<'a> {
        pub topic: &'a str,
        pub limit: u32,
        pub consumer_id: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub skip_sender_id: Option<&'a str>,
    }
    pub async fn recv(args: RecvArgs<'_>) -> Result<Value, RuntimeError> {
        let v = serde_json::to_value(&args)
            .map_err(|e| RuntimeError::new(format!("recv serialize: {e}")))?;
        provider_call("peer", "gossip_recv", v).await
    }

    pub async fn list_topic_peers(topic: &str) -> Result<Value, RuntimeError> {
        provider_call("peer", "list_topic_peers", json!({ "topic": topic })).await
    }
    pub async fn list_peers() -> Result<Value, RuntimeError> {
        provider_call("peer", "list_peers", json!({})).await
    }
    pub async fn get_ticket() -> Result<Value, RuntimeError> {
        provider_call("peer", "get_ticket", json!({})).await
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
// Wire methods (canonical, NAMESPACES.md):
//   content/publish    — store + replicate; returns availability receipt
//   content/fetch      — retrieve bytes by CID (optionally a subpath)
//   content/ensure     — request a replication/pinning policy
//   content/unpublish  — release pin / availability
//
// The function names here keep their old shapes (add_bytes / get_bytes /
// pin / unpin) so callers don't need to all change in one pass — only
// the wire and the response parsing moved. A `pub use content as ipfs`
// alias lives below the module so existing `use crate::runtime::ipfs`
// imports keep compiling during the cutover.

pub mod content {
    use super::{api_base, provider_call, RuntimeError, B64};
    use base64::Engine;
    use serde_json::{json, Value};

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
        B64.decode(b64)
            .map_err(|e| RuntimeError::new(format!("content.fetch base64: {e}")))
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
        format!("{}/ipfs/{}{}", api_base(), super::encode_uri(cid), suffix)
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
// They get the new content-provider wiring transparently. Drop this once
// every caller has been switched to `runtime::content::*`.
pub use content as ipfs;

// ── hey-transcoder (image/video normalization) ──────────────────────
//
// Wraps the hey-transcoder capsule's provider ops. processForUpload
// inspects the MIME type and runs the right transcode pipeline, falling
// through to the original bytes if the capsule isn't installed or
// returns an error. Mirrors the React reference's
// lib/runtime.js `transcoder.processForUpload`.

pub mod transcoder {
    use super::{provider_call, RuntimeError, B64};
    use base64::Engine;
    use serde_json::{json, Value};

    pub struct Processed {
        pub bytes: Vec<u8>,
        pub mime: String,
        pub transcoded: bool,
    }

    pub async fn process_for_upload(
        bytes: &[u8],
        mime: &str,
    ) -> Result<Processed, RuntimeError> {
        let m = mime.to_lowercase();
        let kind = if m.starts_with("image/") {
            "image"
        } else if m.starts_with("video/") {
            "video"
        } else if m.starts_with("audio/") {
            "audio"
        } else {
            return Ok(Processed {
                bytes: bytes.to_vec(),
                mime: mime.into(),
                transcoded: false,
            });
        };
        let op = match kind {
            "image" => "transcode_image",
            "video" => "transcode_video",
            _ => "transcode_voice",
        };
        let body = match kind {
            // AVIF at quality 80 typically reaches ~25-40% smaller files
            // than the equivalent-quality WebP, with better detail
            // retention in photos. If the hey-transcoder capsule's
            // ffmpeg build doesn't have libavif compiled in, the
            // request will return ok:false and we fall through to the
            // original bytes — see the fallback below.
            "image" => json!({
                "data": B64.encode(bytes),
                "target_format": "avif",
                "max_dim": 2048,
                "quality": 80,
                "strip_metadata": true,
            }),
            "video" => json!({
                "data": B64.encode(bytes),
                "target_codec": "h264",
                "max_dim": 1080,
                "crf": 23,
                "fps": 30,
                "preset": "fast",
            }),
            _ => json!({
                "data": B64.encode(bytes),
                "target_codec": "opus",
                "bitrate_k": 64,
                "normalize_lufs": -16,
            }),
        };
        let resp = provider_call("hey-transcoder", op, body).await;
        let Ok(resp) = resp else {
            // Capsule not installed or provider errored — pass through.
            return Ok(Processed {
                bytes: bytes.to_vec(),
                mime: mime.into(),
                transcoded: false,
            });
        };
        if resp.get("ok").and_then(Value::as_bool) == Some(false) {
            return Ok(Processed {
                bytes: bytes.to_vec(),
                mime: mime.into(),
                transcoded: false,
            });
        }
        let data = resp.get("data").and_then(Value::as_str);
        let format = resp.get("format").and_then(Value::as_str);
        let (Some(data), Some(format)) = (data, format) else {
            return Ok(Processed {
                bytes: bytes.to_vec(),
                mime: mime.into(),
                transcoded: false,
            });
        };
        let decoded = B64
            .decode(data)
            .map_err(|e| RuntimeError::new(format!("transcoder base64: {e}")))?;
        Ok(Processed {
            bytes: decoded,
            mime: format!("{kind}/{format}"),
            transcoded: true,
        })
    }
}

// ── DID resolution ────────────────────────────────────────────────────

pub mod did_provider {
    use super::{provider_call, RuntimeError};
    use serde_json::{json, Value};
    pub async fn resolve(did: &str) -> Result<Value, RuntimeError> {
        provider_call("did", "resolve", json!({ "did": did })).await
    }
}

// ── Storage ───────────────────────────────────────────────────────────
//
// Two route shapes, same disk layout. Detected on first call and memoized.
// Per-capsule (Hey/) and shared (.AppData/) paths both go through the same
// dispatcher.

fn route_mode() -> Option<String> {
    SessionStorage::get::<Option<String>>(ROUTE_MODE_KEY)
        .ok()
        .flatten()
}

fn set_route_mode(mode: &str) {
    let _ = SessionStorage::set(ROUTE_MODE_KEY, mode);
}

fn build_storage_url(mode: &str, suffix: &str) -> (String, Value) {
    let s = suffix.trim_start_matches('/');
    if mode == "patch-0002" {
        let url = format!("{}/api/apps/{}/storage/{}", api_base(), CAPSULE_ID, s);
        let headers = if let Some(launch) = home_launch_token() {
            json!({ "x-elastos-home-token": launch })
        } else {
            Value::Null
        };
        return (url, headers);
    }
    // legacy: Hey/<file> → .AppData/LocalHost/Hey/<file>;
    //         .AppData/<rest> stays as .AppData/<rest>
    let legacy = if s.starts_with(&format!("{PRIVATE_NAMESPACE}/")) {
        format!(".AppData/LocalHost/{s}")
    } else {
        s.to_string()
    };
    let url = format!("{}/api/localhost/Users/self/{}", api_base(), legacy);
    // Legacy path carried an Authorization: Bearer header derived from
    // the runtime-token exchange. We now rely on the app-session
    // HttpOnly cookie set by /session/start (or the legacy
    // /runtime-token redemption path); the cookie rides every fetch via
    // `credentials: 'include'` in fetch_raw.
    (url, Value::Null)
}

async fn dispatch_storage(
    suffix: &str,
    method: &str,
    body: Option<String>,
) -> Result<Response, RuntimeError> {
    let _ = redeem_launch_token().await;
    let attempt = |mode: String,
                   suffix: String,
                   method: String,
                   body: Option<String>| async move {
        let (url, mut headers) = build_storage_url(&mode, &suffix);
        if body.is_some() {
            headers["Content-Type"] = Value::String("application/json".into());
        }
        fetch_raw(&url, &method, body, &headers).await
    };

    if let Some(mode) = route_mode() {
        return attempt(mode, suffix.into(), method.into(), body)
            .await
            .map_err(|e| RuntimeError::new(format!("storage fetch: {e:?}")));
    }
    let resp = attempt(
        "patch-0002".into(),
        suffix.into(),
        method.into(),
        body.clone(),
    )
    .await
    .map_err(|e| RuntimeError::new(format!("storage fetch: {e:?}")))?;
    let s = resp.status();
    if s == 401 || s == 403 || s == 404 {
        let legacy = attempt("legacy".into(), suffix.into(), method.into(), body)
            .await
            .map_err(|e| RuntimeError::new(format!("storage fetch: {e:?}")))?;
        let ls = legacy.status();
        if ls < 500 && ls != 401 && ls != 403 {
            set_route_mode("legacy");
            return Ok(legacy);
        }
        set_route_mode("patch-0002");
        return Ok(resp);
    }
    set_route_mode("patch-0002");
    Ok(resp)
}

// ── Per-capsule namespace (under "Hey/") ─────────────────────────────

pub mod storage {
    use super::*;

    fn clean(p: &str) -> String {
        p.trim_start_matches('/').to_string()
    }

    pub async fn read_json(path: &str) -> Result<Option<Value>, RuntimeError> {
        let suffix = format!("{PRIVATE_NAMESPACE}/{}", clean(path));
        let resp = dispatch_storage(&suffix, "GET", None).await?;
        if resp.status() == 404 {
            return Ok(None);
        }
        if !resp.ok() {
            return Err(RuntimeError::with_status(
                format!("storage GET {path}"),
                resp.status(),
            ));
        }
        let v = JsFuture::from(resp.json().unwrap())
            .await
            .map_err(|e| RuntimeError::new(format!("storage read json: {e:?}")))?;
        Ok(Some(serde_wasm_bindgen::from_value(v).unwrap_or(Value::Null)))
    }

    pub async fn write_json(path: &str, value: &Value) -> Result<(), RuntimeError> {
        let suffix = format!("{PRIVATE_NAMESPACE}/{}", clean(path));
        let body = serde_json::to_string(value)
            .map_err(|e| RuntimeError::new(format!("serialize: {e}")))?;
        let resp = dispatch_storage(&suffix, "PUT", Some(body)).await?;
        // The runtime's create-only paths return 412 Precondition Failed
        // on subsequent overwrite attempts. For the feed-index + append
        // pattern that means "this file already existed at write time."
        // The intent of the caller — "make sure the current value is
        // persisted" — is technically not satisfied (the existing file
        // is what survives), but treating 412 as a hard error spams the
        // user with red banners for what is functionally a benign race.
        // Downgrade to a debug log and return Ok so the UI stays quiet.
        if resp.status() == 412 {
            web_sys::console::debug_1(&JsValue::from_str(&format!(
                "[hey-social] PUT {path} hit 412 (create-only); existing value retained"
            )));
            return Ok(());
        }
        if !resp.ok() {
            return Err(RuntimeError::with_status(
                format!("storage PUT {path}"),
                resp.status(),
            ));
        }
        Ok(())
    }

    pub async fn remove(path: &str) -> Result<(), RuntimeError> {
        let suffix = format!("{PRIVATE_NAMESPACE}/{}", clean(path));
        let resp = dispatch_storage(&suffix, "DELETE", None).await?;
        if !resp.ok() && resp.status() != 404 {
            return Err(RuntimeError::with_status(
                format!("storage DELETE {path}"),
                resp.status(),
            ));
        }
        Ok(())
    }
}

// ── Shared namespace (cross-capsule .AppData/*) ──────────────────────

pub async fn shared_write_json(suffix: &str, value: &Value) -> Result<(), RuntimeError> {
    let body = serde_json::to_string(value)
        .map_err(|e| RuntimeError::new(format!("serialize: {e}")))?;
    let resp = dispatch_storage(suffix, "PUT", Some(body)).await?;
    if !resp.ok() {
        return Err(RuntimeError::with_status(
            format!("shared_write_json PUT {suffix}"),
            resp.status(),
        ));
    }
    Ok(())
}

pub async fn shared_read_json(suffix: &str) -> Result<Option<Value>, RuntimeError> {
    let resp = dispatch_storage(suffix, "GET", None).await?;
    if resp.status() == 404 {
        return Ok(None);
    }
    if !resp.ok() {
        return Err(RuntimeError::with_status(
            format!("shared_read_json GET {suffix}"),
            resp.status(),
        ));
    }
    let v = JsFuture::from(resp.json().unwrap())
        .await
        .map_err(|e| RuntimeError::new(format!("shared read json: {e:?}")))?;
    Ok(Some(serde_wasm_bindgen::from_value(v).unwrap_or(Value::Null)))
}

// ── Misc helpers ─────────────────────────────────────────────────────

fn log_warn(s: &str) {
    web_sys::console::warn_1(&JsValue::from_str(s));
}

fn encode_uri(s: &str) -> String {
    js_sys::encode_uri_component(s).as_string().unwrap_or_default()
}

async fn sleep_ms(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let _ = window()
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
    });
    let _ = JsFuture::from(promise).await;
}

// ── Boot-time capability acquisition (parallel) ──────────────────────

pub async fn acquire_boot_capabilities() {
    let wants: [(&str, &str); 5] = [
        ("elastos://peer/*", "message"),
        ("elastos://content/*", "write"),
        ("elastos://did/*", "read"),
        ("elastos://hey-transcoder/*", "execute"),
        ("elastos://elacity/*", "execute"),
    ];
    for (resource, action) in wants {
        let _ = ensure_capability_token(resource, action).await;
    }
}

// ── Wallet-style session inheritance (Home → app SSO) ─────────────────
//
// Upstream's app-launch contract (per state.md / Chat Room reference):
//
//   1. Home authenticates the user via passkey.
//   2. Home launches the app with `?home_token=<signed-launch-token>`.
//   3. The app POSTs that token to /api/apps/<capsule>/session/start
//      (or /runtime-token — both flavors of the same redemption) with
//      `x-elastos-home-token: <token>`. The bearer_ready() flow above
//      already does this leg.
//   4. The app then reads who the user is via GET /api/session.
//   5. The app scrubs ?home_token=... from its visible URL so the token
//      doesn't leak via screenshots, bookmarks, or history.
//
// This is what makes wallet "just work" when launched from Home: it
// never runs its own passkey ceremony, it just rides the launch token
// into an app-scoped session and then asks the runtime "who am I?"
//
// Hey-social does step 3 (bearer_ready), this section adds steps 4–5.

/// Strip ?home_token=... and ?runtime_token=... from the visible URL
/// after the launch token has been redeemed. Equivalent to:
///   history.replaceState({}, "", location.pathname);
/// but only touches the two query params we know about — preserves any
/// other query params and the hash fragment. Idempotent.
pub fn scrub_launch_token_from_url() {
    let win = match web_sys::window() {
        Some(w) => w,
        None => return,
    };
    let loc = win.location();
    let search = loc.search().unwrap_or_default();
    if !search.contains("home_token") && !search.contains("runtime_token") {
        return;
    }
    let pathname = loc.pathname().unwrap_or_default();
    let hash = loc.hash().unwrap_or_default();
    let trimmed = search.trim_start_matches('?');
    let params = match web_sys::UrlSearchParams::new_with_str(trimmed) {
        Ok(p) => p,
        Err(_) => return,
    };
    params.delete("home_token");
    params.delete("runtime_token");
    let new_search: String = params.to_string().into();
    let new_url = if new_search.is_empty() {
        format!("{pathname}{hash}")
    } else {
        format!("{pathname}?{new_search}{hash}")
    };
    if let Ok(history) = win.history() {
        let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&new_url));
    }
}

/// Inherit the user identity from the runtime session — the wallet-style
/// path. After redeem_launch_token() has redeemed the launch token, we
/// ask /api/session who's signed in and bootstrap a thin local Session
/// (DID + name only, no signing key). The local Session.auth_key_hex
/// stays empty; the existing passkey ceremony will fill it on demand
/// when the user takes their first signing action (post / DM / follow).
/// Read-only flows (browsing the feed, viewing profiles) just work
/// from the inherited identity.
///
/// **Stock upstream returns None.** `GET /api/session` on upstream
/// returns `SessionInfoOutput { session_id, session_type, vm_id,
/// capabilities_count, created_at, last_active }` — session metadata,
/// not user identity (confirmed dev 2026-05-29; source:
/// `handlers/capability.rs:527-572`). So this function always falls
/// through to None on stock upstream, and Landing falls back to the
/// passkey ceremony. This is the "transitional empty auth_key_hex +
/// lazy passkey" path the dev framing endorses for unblocking, NOT a
/// long-term design.
///
/// For inherit_session to actually populate a session, ONE of:
///   - Runtime extends /api/session to include identity (out of our
///     reach; document and wait)
///   - A new identity provider answers `elastos://did/sign` or
///     `elastos://identity/whoami` with the projected DID. The
///     [identity-projection-provider](../identity-projection-provider/)
///     in this pack has the wire shape; needs scheme dispatch to
///     actually run.
///
/// Returns None if no inherited session is available. Probes the
/// payload defensively in case a YNH-patched or future upstream
/// build does include identity fields.
pub async fn inherit_session() -> Option<crate::session::Session> {
    let raw = session_current().await?;
    // The session payload's exact shape isn't fixed across runtime
    // versions, so probe a few common field paths defensively. CRITICAL
    // ORDERING: we look for SOCIAL-DID-shaped fields first (`didKey`,
    // `did_key`, `did`) and INTENTIONALLY skip `principal`. Per the
    // upstream ontology a `principal` is the runtime user handle (e.g.
    // `person:local:…`), NOT the social federated identity. Even if a
    // future principal happens to start with `did:`, it would still be
    // the runtime principal, not the user's social DID — and using it
    // as the social DID would re-create the "person:local:… shows up
    // as my did:key" bug from the messaging audit.
    let did = first_str(&raw, &[
        &["didKey"],
        &["did_key"],
        &["did"],
        &["user", "didKey"],
        &["user", "did_key"],
        &["user", "did"],
        &["identity", "didKey"],
        &["identity", "did"],
    ])
    .filter(|s| s.starts_with("did:"))?;
    let name = first_str(&raw, &[
        &["name"],
        &["display_name"],
        &["displayName"],
        &["user", "name"],
        &["user", "display_name"],
        &["user", "displayName"],
        &["identity", "name"],
    ])
    .unwrap_or_else(|| short_did_name(&did));
    Some(crate::session::Session {
        auth_key_hex: String::new(),
        did_key: did,
        name,
        ml_kem_secret_b64: String::new(),
        ml_kem_public_b64: String::new(),
    })
}

fn first_str(v: &Value, paths: &[&[&str]]) -> Option<String> {
    for path in paths {
        let mut cur = v;
        let mut ok = true;
        for key in *path {
            match cur.get(*key) {
                Some(next) => cur = next,
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            if let Some(s) = cur.as_str() {
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

fn short_did_name(did: &str) -> String {
    // Fallback display name when the runtime didn't hand us one — show
    // the last 6 characters of the DID so it's not a totally opaque blob.
    if did.len() > 10 {
        format!("user-{}", &did[did.len() - 6..])
    } else {
        did.to_string()
    }
}

// ── Session introspection ────────────────────────────────────────────
//
// Upstream's runtime exposes the per-capsule session through
// `GET /api/session` (handlers/capability.rs:541 — returns
// `{session_id, session_type, vm_id, capabilities_count, created_at,
// last_active}`). There ARE three reserved scheme names —
// `elastos://session/*`, `elastos://principal/*`,
// `elastos://capabilities/*` — but NO built-in capsule registers them
// in upstream @ 6d4c385. Calling `provider_call("session", ...)` today
// is guaranteed to fail (the scheme has no provider), so we don't
// bother — saves a round-trip per session lookup. If a session
// provider lands upstream later, swap to it here.

pub async fn session_current() -> Option<Value> {
    let _ = redeem_launch_token().await;
    let headers = json!({});
    let url = api_url("/api/session");
    let resp = fetch_raw(&url, "GET", None, &headers).await.ok()?;
    if !resp.ok() {
        return None;
    }
    let v = JsFuture::from(resp.json().ok()?).await.ok()?;
    Some(serde_wasm_bindgen::from_value(v).unwrap_or(Value::Null))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedIdentity {
    pub name: String,
    #[serde(rename = "didKey")]
    pub did_key: String,
    #[serde(rename = "recoveryKeyHash")]
    pub recovery_key_hash: String,
    pub passkeys: Vec<Value>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "createdBy")]
    pub created_by: String,
}
