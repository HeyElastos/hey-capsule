// Signed federation events — Rust port of capsules/hey-social/client/src/lib/events.js.
//
// Same on-wire shape (type, payload, sender_did, ts, signature) and the same
// sorted-keys canonicalization so signatures survive JSON wire round-trips
// between Rust + JS senders/receivers.
//
// SECURITY NOTE: The React reference keeps the Ed25519 seed as a non-
// extractable Web Crypto CryptoKey in IndexedDB after sign-in (see
// lib/keystore.js). The Rust port currently stores the auth-key hex in
// localStorage and signs with ed25519-compact in-process. Hardening to
// match the React hardened path is a TODO — opens an IndexedDB-backed
// keystore and switches to crypto.subtle.sign({name:"Ed25519"}) via
// web-sys. Not blocking for v0.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::identity::{
    bytes_to_hex, did_key_to_public_key, hex_to_bytes, public_key_to_did_key, sign, verify,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub payload: Value,
    pub sender_did: String,
    pub ts: i64,
    pub signature: String,
}

// Canonicalize a JSON Value with sorted object keys. Required so the bytes
// signed here match the bytes verified anywhere else after JSON round-trips.
pub fn canonicalize(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            // Rust's float formatter writes "1.0" where JS JSON.stringify
            // would write "1". For a payload signed in Rust + verified
            // in JS that's a silent signature mismatch. We only allow
            // integer Numbers (i64/u64) through canonicalize — any
            // float gets normalized to its integer cast when lossless,
            // else stringified to "0" with a console warning so the
            // signature path stays deterministic across senders.
            if let Some(i) = n.as_i64() {
                out.push_str(&i.to_string());
            } else if let Some(u) = n.as_u64() {
                out.push_str(&u.to_string());
            } else if let Some(f) = n.as_f64() {
                if f.is_finite() && f.fract() == 0.0 && f.abs() < (i64::MAX as f64) {
                    out.push_str(&(f as i64).to_string());
                } else {
                    web_sys::console::warn_1(&wasm_bindgen::JsValue::from_str(
                        "[hey-social] canonicalize: non-integer Number coerced to 0 to preserve cross-language signature determinism",
                    ));
                    out.push('0');
                }
            } else {
                out.push('0');
            }
        }
        Value::String(s) => {
            // serde_json::to_string handles escape-quoting consistently with
            // JS JSON.stringify for the subset of strings Hey events carry.
            out.push_str(&serde_json::to_string(s).unwrap_or_default());
        }
        Value::Array(arr) => {
            out.push('[');
            for (i, v) in arr.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(v, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(k).unwrap_or_default());
                out.push(':');
                write_canonical(&map[*k], out);
            }
            out.push('}');
        }
    }
}

fn bytes_to_sign(event_type: &str, payload: &Value, sender_did: &str, ts: i64) -> String {
    canonicalize(&serde_json::json!({
        "type": event_type,
        "payload": payload,
        "sender_did": sender_did,
        "ts": ts,
    }))
}

// Construct a signed envelope from the user's auth-key hex + event body.
// Caller (api/posts.rs etc.) is responsible for unwrapping session::current()
// first so this stays a pure function.
pub fn create_signed_event(
    event_type: &str,
    payload: Value,
    auth_key_hex: &str,
) -> Result<SignedEvent, String> {
    if event_type.is_empty() {
        return Err("event.type is required".into());
    }
    let seed_vec = hex_to_bytes(auth_key_hex)?;
    if seed_vec.len() != 32 {
        return Err("auth_key must be 32 bytes (64 hex chars)".into());
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&seed_vec);
    // Recover the public key from the seed for the sender_did claim.
    let kp = ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new(seed));
    let pk_bytes: [u8; 32] = *kp.pk;
    let sender_did = public_key_to_did_key(&pk_bytes);
    let ts = (js_sys::Date::now() as i64).max(0);
    let message = bytes_to_sign(event_type, &payload, &sender_did, ts);
    let signature = sign(message.as_bytes(), &seed);
    Ok(SignedEvent {
        event_type: event_type.into(),
        payload,
        sender_did,
        ts,
        signature,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyResult {
    Valid,
    Invalid(&'static str),
}

// Verify a received event. Never panics on malformed input (DoS-safe).
pub fn verify_signed_event(event: &SignedEvent) -> VerifyResult {
    if event.event_type.is_empty() {
        return VerifyResult::Invalid("bad-type");
    }
    if !event.sender_did.starts_with("did:key:z") {
        return VerifyResult::Invalid("bad-sender_did");
    }
    if event.ts <= 0 {
        return VerifyResult::Invalid("bad-ts");
    }
    if event.signature.len() != 128 {
        return VerifyResult::Invalid("bad-signature-shape");
    }
    let pk = match did_key_to_public_key(&event.sender_did) {
        Ok(p) => p,
        Err(_) => return VerifyResult::Invalid("unresolvable-did"),
    };
    let msg = bytes_to_sign(&event.event_type, &event.payload, &event.sender_did, event.ts);
    if verify(msg.as_bytes(), &event.signature, &pk) {
        VerifyResult::Valid
    } else {
        VerifyResult::Invalid("signature-mismatch")
    }
}

// Convenience: serialize for `peer.publish.message`.
pub fn to_wire_string(event: &SignedEvent) -> String {
    serde_json::to_string(event).unwrap_or_default()
}

// Deserialize a wire string back to a SignedEvent (Carrier message body).
pub fn from_wire_string(s: &str) -> Option<SignedEvent> {
    serde_json::from_str(s).ok()
}

// Re-export for tests / future hardening.
pub fn _bytes_to_hex(b: &[u8]) -> String {
    bytes_to_hex(b)
}
