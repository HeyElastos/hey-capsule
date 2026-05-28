// Passkey sign-in — Rust port of signInViaRuntime from
// capsules/hey-social/client/src/api/passkey.js.
//
// Flow (must stay byte-identical with the React + messenger versions —
// upstream's deserializer uses #[serde(deny_unknown_fields)] everywhere,
// so any field-name drift fails with HTTP 422):
//
//   1. POST /api/auth/passkey/authenticate/begin    → options
//   2. Inject cross-capsule PRF extension          (window.heyPasskeyAuthenticate
//                                                   passes the options into the
//                                                   browser's WebAuthn API)
//   3. POST /api/auth/passkey/authenticate/complete with the canonical
//      four-layer envelope (outer ceremony_id+response; assertion
//      id+rawId+type+response; inner clientDataJson [lowercase j]
//      +authenticatorData+signature+userHandle).
//   4. Decode PRF output, derive Ed25519 keypair via identity::expand_keypair,
//      stash session locally.
//   5. Dual-write shared identity (.AppData/ElastOS/Identity/profile.json
//      + .AppData/Identity/profile.json) so other Hey capsules adopt this user.

use js_sys::{Promise, Reflect};
use serde_json::{json, Value};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::Response;

use crate::identity::{
    bytes_to_hex, expand_keypair, ELASTOS_IDENTITY_PRF_INPUT,
};
use crate::runtime::{shared_write_json, upstream_fetch, RuntimeError};
use crate::session::{self, Session};

pub fn passkey_supported() -> bool {
    let win = match web_sys::window() {
        Some(w) => w,
        None => return false,
    };
    let f = match Reflect::get(&win, &JsValue::from_str("heyPasskeySupported")) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if !f.is_function() {
        return false;
    }
    let func: js_sys::Function = f.unchecked_into();
    func.call0(&win)
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

// SimpleWebAuthn / our shim surfaces PRF outputs as base64url-encoded strings.
// Returns 32 raw bytes or None.
fn decode_b64u(s: &str) -> Option<Vec<u8>> {
    let pad = (4 - s.len() % 4) % 4;
    let s = s.replace('-', "+").replace('_', "/");
    let s = format!("{}{}", s, "=".repeat(pad));
    // RFC 4648 base64 (standard) — small hand-roll to avoid a base64 dep.
    let alphabet: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut table = [255u8; 256];
    for (i, b) in alphabet.iter().enumerate() {
        table[*b as usize] = i as u8;
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for &c in bytes {
        if c == b'=' {
            break;
        }
        let v = table[c as usize];
        if v == 255 {
            return None;
        }
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

fn prf_identity_from_assertion(assertion: &Value) -> Option<Vec<u8>> {
    let first = assertion
        .get("clientExtensionResults")?
        .get("prf")?
        .get("results")?
        .get("first")?
        .as_str()?;
    let bytes = decode_b64u(first)?;
    if bytes.len() == 32 {
        Some(bytes)
    } else {
        None
    }
}

async fn response_json(resp: Response) -> Result<Value, RuntimeError> {
    let promise = resp
        .json()
        .map_err(|e| RuntimeError::new(format!("response.json(): {e:?}")))?;
    let v = JsFuture::from(promise)
        .await
        .map_err(|e| RuntimeError::new(format!("response.json() await: {e:?}")))?;
    serde_wasm_bindgen::from_value(v).map_err(|e| RuntimeError::new(format!("json decode: {e}")))
}

async fn response_text(resp: Response) -> String {
    match resp.text() {
        Ok(p) => match JsFuture::from(p).await {
            Ok(v) => v.as_string().unwrap_or_default(),
            Err(_) => String::new(),
        },
        Err(_) => String::new(),
    }
}

pub async fn sign_in_via_runtime(nickname: Option<String>) -> Result<Session, String> {
    // 1. Ask upstream for authenticate options. POST first, fall back to GET
    //    on 405 (upstream's older v0.2 builds expose it as GET).
    let mut begin_resp = upstream_fetch(
        "/api/auth/passkey/authenticate/begin",
        "POST",
        Some("{}".into()),
    )
    .await
    .map_err(|e| e.to_string())?;
    if begin_resp.status() == 405 {
        begin_resp = upstream_fetch("/api/auth/passkey/authenticate/begin", "GET", None)
            .await
            .map_err(|e| e.to_string())?;
    }
    if !begin_resp.ok() {
        if begin_resp.status() == 400 || begin_resp.status() == 404 {
            return Err(
                "No passkey set up on this device yet. Go back to System and create a passkey first, then come back here."
                    .into(),
            );
        }
        return Err(format!(
            "passkey authenticate/begin: HTTP {}",
            begin_resp.status()
        ));
    }
    let begin_json = response_json(begin_resp).await.map_err(|e| e.to_string())?;
    web_sys::console::info_1(&JsValue::from_str(&format!(
        "[hey-social-rust] /authenticate/begin: {}",
        begin_json
    )));

    let ceremony_id = begin_json
        .get("ceremony_id")
        .or_else(|| begin_json.get("ceremonyId"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Extract the WebAuthn PublicKeyCredentialRequestOptions out of the
    // /authenticate/begin response. Multiple known upstream shapes:
    //
    //   v0.3a (newer builds — confirmed against elastos.app 2026-05-28):
    //     { options: { challenge, rpId, timeout, userVerification,
    //                  publicKey: <allowCredentials[]> } }
    //     — `options` IS the WebAuthn options object; `options.publicKey`
    //       is the allowCredentials array (badly named upstream-side).
    //
    //   v0.3b (older builds — what the React passkey.js comment says):
    //     { options: { publicKey: <full WebAuthn options object> } }
    //
    //   v0.2 fallback:  { publicKey: <options> }  or  bare options at root.
    //
    // Strategy: pick the first candidate that already has `challenge` at
    // its top level. For v0.3a, rename `publicKey` → `allowCredentials`
    // so the WebAuthn shim downstream sees a standard-shape options object.
    let candidates: Vec<serde_json::Value> = [
        begin_json.get("options").cloned(),
        begin_json
            .get("options")
            .and_then(|o| o.get("publicKey").cloned()),
        begin_json.get("publicKey").cloned(),
        Some(begin_json.clone()),
    ]
    .into_iter()
    .flatten()
    .collect();

    let mut options = candidates
        .into_iter()
        .find(|v| {
            v.is_object()
                && v.get("challenge")
                    .map(|c| !c.is_null())
                    .unwrap_or(false)
        })
        .ok_or_else(|| {
            "passkey authenticate/begin response missing 'challenge' — upstream contract mismatch."
                .to_string()
        })?;

    // v0.3a fixup: if options.publicKey is an array, rename to allowCredentials
    // so the WebAuthn shim's standard handling kicks in.
    if let Some(obj) = options.as_object_mut() {
        if let Some(pk) = obj.get("publicKey").cloned() {
            if pk.is_array() {
                obj.remove("publicKey");
                obj.insert("allowCredentials".to_string(), pk);
            }
        }
    }

    // 2. Inject the cross-capsule unified-identity PRF extension. We send
    //    the input base64url-encoded; the JS shim decodes it back to bytes
    //    before calling navigator.credentials.get.
    let prf_input_b64u = {
        let bytes: Vec<u8> = ELASTOS_IDENTITY_PRF_INPUT.to_vec();
        let pad_stripped = b64u_encode(&bytes);
        pad_stripped
    };
    {
        let exts = options
            .as_object_mut()
            .unwrap()
            .entry("extensions")
            .or_insert_with(|| json!({}));
        if let Some(obj) = exts.as_object_mut() {
            obj.entry("prf").or_insert_with(|| {
                json!({ "eval": { "first": prf_input_b64u } })
            });
        }
    }

    // 3. Hand off to the JS shim → navigator.credentials.get(...).
    let assertion = run_webauthn(&options).await?;
    web_sys::console::info_1(&JsValue::from_str(&format!(
        "[hey-social-rust] assertion: {}",
        assertion
    )));

    // 4. Build the canonical 4-layer envelope. Field names + serde-case
    //    are load-bearing — see reference_passkey_contract memory.
    let inner_response = json!({
        "clientDataJson": assertion.get("response").and_then(|r| r.get("clientDataJSON")).cloned().unwrap_or(Value::Null),
        "authenticatorData": assertion.get("response").and_then(|r| r.get("authenticatorData")).cloned().unwrap_or(Value::Null),
        "signature": assertion.get("response").and_then(|r| r.get("signature")).cloned().unwrap_or(Value::Null),
        "userHandle": assertion.get("response").and_then(|r| r.get("userHandle")).cloned().unwrap_or(Value::Null),
    });
    let normalized = json!({
        "id": assertion.get("id").cloned().unwrap_or(Value::Null),
        "rawId": assertion.get("rawId").cloned().unwrap_or(Value::Null),
        "type": assertion.get("type").and_then(|v| v.as_str()).unwrap_or("public-key"),
        "response": inner_response,
    });
    let complete_body = if let Some(cid) = ceremony_id.as_ref() {
        json!({ "ceremony_id": cid, "response": normalized })
    } else {
        normalized
    };

    let complete_resp = upstream_fetch(
        "/api/auth/passkey/authenticate/complete",
        "POST",
        Some(complete_body.to_string()),
    )
    .await
    .map_err(|e| e.to_string())?;
    if !complete_resp.ok() {
        let status = complete_resp.status();
        let txt = response_text(complete_resp).await;
        return Err(format!(
            "passkey authenticate/complete: HTTP {} {}",
            status,
            txt.chars().take(200).collect::<String>()
        ));
    }
    let upstream_result: Value = response_json(complete_resp).await.unwrap_or(Value::Null);

    // 5. Derive the signing identity from the PRF output. No PRF → no
    //    deterministic DID → we refuse rather than fall back to a random
    //    key (a randomly-keyed identity defeats the cross-capsule
    //    continuity that's the whole point of this flow).
    let identity_prf = prf_identity_from_assertion(&assertion).ok_or_else(|| {
        "Passkey didn't return PRF output — your authenticator lacks the prf extension. \
         Use a PRF-capable passkey (Yubikey 5.7+, Touch ID macOS 14+, modern Windows Hello, Android 14+)."
            .to_string()
    })?;
    let auth_key = bytes_to_hex(&identity_prf);
    let expanded = expand_keypair(&auth_key).map_err(|e| format!("expand_keypair: {e}"))?;
    let did_key = expanded.did_key;

    let upstream_name = upstream_result
        .get("displayName")
        .or_else(|| upstream_result.get("name"))
        .or_else(|| upstream_result.get("user").and_then(|u| u.get("name")))
        .or_else(|| upstream_result.get("user").and_then(|u| u.get("displayName")))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let name = nickname
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or(upstream_name)
        .unwrap_or_else(|| format!("{}…", did_key.chars().take(14).collect::<String>()));

    // Reuse existing ML-KEM keypair if present (so re-signin on the same
    // device doesn't change our publicly-published KEM pub). Otherwise
    // generate a fresh one — 2.4 KB secret, ~1.6 KB public, b64-encoded.
    let (ml_kem_secret_b64, ml_kem_public_b64) = match session::current() {
        Some(prev) if !prev.ml_kem_secret_b64.is_empty() => {
            (prev.ml_kem_secret_b64, prev.ml_kem_public_b64)
        }
        _ => {
            use base64::engine::general_purpose::STANDARD as B64;
            use base64::Engine as _;
            let (sk, pk) = crate::crypto::generate_ml_kem_keypair();
            (B64.encode(&sk), B64.encode(&pk))
        }
    };

    let new_session = Session {
        auth_key_hex: auth_key,
        did_key: did_key.clone(),
        name: name.clone(),
        ml_kem_secret_b64,
        ml_kem_public_b64,
    };
    session::set(&new_session);

    // 6. Shared-identity dual-write so other Hey capsules (home shell,
    //    Hey Social, Hey Messenger) see this user as already signed up.
    //    Non-fatal on failure — the user is signed in locally either way.
    let profile = json!({
        "name": name,
        "didKey": did_key,
        "recoveryKeyHash": "",
        "passkeys": [],
        "createdAt": chrono_now(),
        "createdBy": "hey-social-rust-runtime-signin",
    });
    let _ = shared_write_json(".AppData/ElastOS/Identity/profile.json", &profile).await;
    let _ = shared_write_json(".AppData/Identity/profile.json", &profile).await;

    Ok(new_session)
}

fn b64u_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = Vec::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let n = (bytes[i] as u32) << 16 | (bytes[i + 1] as u32) << 8 | bytes[i + 2] as u32;
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize]);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize]);
        out.push(ALPHABET[((n >> 6) & 0x3f) as usize]);
        out.push(ALPHABET[(n & 0x3f) as usize]);
        i += 3;
    }
    let rem = bytes.len() - i;
    if rem == 1 {
        let n = (bytes[i] as u32) << 16;
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize]);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize]);
    } else if rem == 2 {
        let n = (bytes[i] as u32) << 16 | (bytes[i + 1] as u32) << 8;
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize]);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize]);
        out.push(ALPHABET[((n >> 6) & 0x3f) as usize]);
    }
    String::from_utf8(out).unwrap()
}

// ISO 8601 "now" — js_sys::Date::new_0().to_iso_string() returns "...Z".
fn chrono_now() -> String {
    js_sys::Date::new_0()
        .to_iso_string()
        .as_string()
        .unwrap_or_default()
}

// Call the JS shim (defined in index.html) which wraps navigator.credentials.get
// with the base64url encoding/decoding simplewebauthn provides on the JS side.
//
// CRITICAL: serde_wasm_bindgen::to_value's default serializer emits a JS `Map`
// for serde_json::Value::Object, not a plain object. The shim accesses
// `options.challenge` (dot-notation property), which returns undefined on a
// Map. Use `Serializer::new().serialize_maps_as_objects(true)` so the shim
// sees a normal {…} object.
async fn run_webauthn(options: &Value) -> Result<Value, String> {
    use serde::Serialize as _;
    let win = web_sys::window().ok_or("no window")?;
    let func_val = Reflect::get(&win, &JsValue::from_str("heyPasskeyAuthenticate"))
        .map_err(|e| format!("Reflect.get heyPasskeyAuthenticate: {e:?}"))?;
    if !func_val.is_function() {
        return Err("WebAuthn shim missing — heyPasskeyAuthenticate not defined".into());
    }
    let func: js_sys::Function = func_val.unchecked_into();
    let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
    let opts_js = options
        .serialize(&serializer)
        .map_err(|e| format!("options → JS: {e}"))?;
    let result = func
        .call1(&win, &opts_js)
        .map_err(|e| webauthn_error_message(&e))?;
    let promise: Promise = result.dyn_into().map_err(|_| "shim did not return Promise".to_string())?;
    let assertion_js = JsFuture::from(promise)
        .await
        .map_err(|e| webauthn_error_message(&e))?;
    serde_wasm_bindgen::from_value(assertion_js).map_err(|e| format!("assertion → JSON: {e}"))
}

fn webauthn_error_message(e: &JsValue) -> String {
    // DOMException carries a name we can map to a friendly message; fall
    // back to the stringified value otherwise.
    if let Ok(name) = Reflect::get(e, &JsValue::from_str("name")) {
        if let Some(n) = name.as_string() {
            if n == "NotAllowedError" || n == "AbortError" {
                return "Passkey prompt closed. Tap to try again.".into();
            }
            if let Ok(msg) = Reflect::get(e, &JsValue::from_str("message")) {
                if let Some(m) = msg.as_string() {
                    return format!("{n}: {m}");
                }
            }
            return n;
        }
    }
    format!("{e:?}")
}
