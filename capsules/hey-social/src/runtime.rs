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

const RUNTIME_TOKEN_KEY: &str = "hey-runtime-token";
const HOME_LAUNCH_TOKEN_KEY: &str = "hey-home-launch-token";
const ROUTE_MODE_KEY: &str = "hey-storage-route-mode";
const TOKEN_STORE_KEY: &str = "hey-capability-tokens";

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
                let _ = SessionStorage::delete(RUNTIME_TOKEN_KEY);
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

pub async fn bearer_ready() -> bool {
    if let Ok(Some(_existing)) = SessionStorage::get::<Option<String>>(RUNTIME_TOKEN_KEY) {
        return true;
    }
    let Some(launch) = home_launch_token() else {
        return false;
    };
    let url = api_url(&format!("/api/apps/{CAPSULE_ID}/runtime-token"));
    let headers = json!({
        "Content-Type": "application/json",
        "x-elastos-home-token": launch,
    });
    match fetch_raw(&url, "POST", Some("{}".to_string()), &headers).await {
        Ok(resp) => {
            if !resp.ok() {
                log_warn(&format!(
                    "[hey-social] runtime-token exchange failed: {}",
                    resp.status()
                ));
                return false;
            }
            match JsFuture::from(resp.json().unwrap()).await {
                Ok(v) => {
                    let json: Value = serde_wasm_bindgen::from_value(v).unwrap_or(Value::Null);
                    if let Some(tok) = json.get("token").and_then(|t| t.as_str()) {
                        let _ = SessionStorage::set(RUNTIME_TOKEN_KEY, tok);
                        return true;
                    }
                    false
                }
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}

fn current_runtime_token() -> Option<String> {
    SessionStorage::get::<Option<String>>(RUNTIME_TOKEN_KEY)
        .ok()
        .flatten()
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
    let resp_value = JsFuture::from(window().fetch_with_request(&req)).await?;
    resp_value.dyn_into::<Response>()
}

// Public helper used by passkey.rs to call upstream's /api/auth/passkey/*
// endpoints. Always carries the session cookie; carries the bearer header
// once bearer_ready() has resolved (idempotent — safe to call on every hit).
pub async fn upstream_fetch(
    path: &str,
    method: &str,
    body: Option<String>,
) -> Result<Response, RuntimeError> {
    let _ = bearer_ready().await;
    let mut headers = json!({ "Content-Type": "application/json" });
    if let Some(tok) = current_runtime_token() {
        headers["Authorization"] = Value::String(format!("Bearer {tok}"));
    }
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
    let _ = bearer_ready().await;
    let mut headers = json!({ "Content-Type": "application/json" });
    if let Some(tok) = current_runtime_token() {
        headers["Authorization"] = Value::String(format!("Bearer {tok}"));
    }
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
        let mut poll_headers = json!({});
        if let Some(tok) = current_runtime_token() {
            poll_headers["Authorization"] = Value::String(format!("Bearer {tok}"));
        }
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
    if let Some(tok) = current_runtime_token() {
        headers["Authorization"] = Value::String(format!("Bearer {tok}"));
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

// ── IPFS (media storage via Kubo) ─────────────────────────────────────

pub mod ipfs {
    use super::{api_base, provider_call, RuntimeError, B64};
    use base64::Engine;
    use serde_json::{json, Value};

    pub async fn add_bytes(
        bytes: &[u8],
        filename: &str,
        pin: bool,
    ) -> Result<Value, RuntimeError> {
        let body = json!({
            "data": B64.encode(bytes),
            "filename": filename,
            "pin": pin,
        });
        provider_call("ipfs", "add_bytes", body).await
    }

    pub async fn get_bytes(cid: &str, path: Option<&str>) -> Result<Vec<u8>, RuntimeError> {
        let mut body = json!({ "cid": cid });
        if let Some(p) = path {
            body["path"] = Value::String(p.into());
        }
        let resp = provider_call("ipfs", "get_bytes", body).await?;
        let b64 = resp
            .get("data")
            .and_then(|d| d.get("data"))
            .and_then(|d| d.as_str())
            .ok_or_else(|| RuntimeError::new(format!("ipfs.get_bytes({cid}): no data in response")))?;
        B64.decode(b64)
            .map_err(|e| RuntimeError::new(format!("ipfs.get_bytes base64: {e}")))
    }

    // The IPFS gateway is proxied by nginx at /<API_BASE>/ipfs/<CID>; CIDs are
    // content-addressed so possession of the CID is itself the access token,
    // making this safe for direct <img> src binding (which can't carry headers).
    pub fn gateway_url(cid: &str, path: Option<&str>) -> String {
        let suffix = match path {
            Some(p) => format!("/{}", p.trim_start_matches('/')),
            None => String::new(),
        };
        format!("{}/ipfs/{}{}", api_base(), super::encode_uri(cid), suffix)
    }

    pub async fn pin(cid: &str) -> Result<Value, RuntimeError> {
        provider_call("ipfs", "pin", json!({ "cid": cid })).await
    }
    pub async fn unpin(cid: &str) -> Result<Value, RuntimeError> {
        provider_call("ipfs", "unpin", json!({ "cid": cid })).await
    }
    pub async fn ls(cid: &str) -> Result<Value, RuntimeError> {
        provider_call("ipfs", "ls", json!({ "cid": cid })).await
    }
    pub async fn health() -> Result<Value, RuntimeError> {
        provider_call("ipfs", "health", json!({})).await
    }
}

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
    let headers = if let Some(tok) = current_runtime_token() {
        json!({ "Authorization": format!("Bearer {tok}") })
    } else {
        Value::Null
    };
    (url, headers)
}

async fn dispatch_storage(
    suffix: &str,
    method: &str,
    body: Option<String>,
) -> Result<Response, RuntimeError> {
    let _ = bearer_ready().await;
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
        ("elastos://ipfs/*", "write"),
        ("elastos://did/*", "read"),
        ("elastos://hey-transcoder/*", "execute"),
        ("elastos://elacity/*", "execute"),
    ];
    for (resource, action) in wants {
        let _ = ensure_capability_token(resource, action).await;
    }
}

// ── Session / passkey status (upstream introspection) ─────────────────

pub async fn session_current() -> Option<Value> {
    let _ = bearer_ready().await;
    let mut headers = json!({});
    if let Some(tok) = current_runtime_token() {
        headers["Authorization"] = Value::String(format!("Bearer {tok}"));
    }
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
