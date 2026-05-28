// Direct-message API with E2E hybrid post-quantum encryption.
//
// Wire format (sent via peer.publish, received via peer.recv → routed by
// peer_receiver):
//
//   type: "dm.message"
//   payload: {
//     sender_pubkeys: { x25519_pub_b64, ml_kem_pub_b64 },
//     // ONE OF:
//     envelope: { v, eph, kem, n, ct },   // encrypted (preferred)
//     text: "..."                          // plaintext fallback for the
//                                          // very first message before
//                                          // we've seen the peer's pubkeys
//   }
//
// Bootstrap: the FIRST message between two users is plaintext because
// neither side has the other's pubkeys yet. Recipients cache the
// sender_pubkeys on receive — subsequent messages in either direction
// use the cached keys + envelope.
//
// Storage:
//   Hey/dm/contacts.json    — [ { did, name, lastTs, lastPreview, unread } ]
//   Hey/dm/by-did/<did>.json — [ { id, text, ts, mine } ]
//   Hey/dm/peer-keys.json   — { did: { x25519_pub_b64, ml_kem_pub_b64 } }

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::api::profile::ensure_profile;
use crate::crypto::{self, HpqEnvelope, UserKeys};
use crate::events::create_signed_event;
use crate::identity::hex_to_bytes;
use crate::runtime::{peer, storage, RuntimeError};
use crate::session;

const CONTACTS_FILE: &str = "dm/contacts.json";
const PEER_KEYS_FILE: &str = "dm/peer-keys.json";

fn conv_path(did: &str) -> String {
    let safe = did.replace(['/', ':'], "_");
    format!("dm/by-did/{safe}.json")
}

fn now_ms() -> i64 {
    js_sys::Date::now() as i64
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmContact {
    pub did: String,
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "lastTs")]
    pub last_ts: i64,
    #[serde(default, rename = "lastPreview")]
    pub last_preview: String,
    #[serde(default)]
    pub unread: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmMessage {
    pub id: String,
    pub text: String,
    pub ts: i64,
    pub mine: bool,
    /// True if this message was delivered through the E2E envelope path,
    /// false if it was a plaintext bootstrap. The UI can surface a small
    /// "encrypted" or "unencrypted" hint accordingly.
    #[serde(default)]
    pub encrypted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerKeys {
    pub x25519_pub_b64: String,
    pub ml_kem_pub_b64: String,
}

pub async fn list_contacts() -> Vec<DmContact> {
    storage::read_json(CONTACTS_FILE)
        .await
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value::<Vec<DmContact>>(v).ok())
        .unwrap_or_default()
}

async fn write_contacts(list: &[DmContact]) -> Result<(), RuntimeError> {
    let v = serde_json::to_value(list)
        .map_err(|e| RuntimeError::new(format!("serialize: {e}")))?;
    storage::write_json(CONTACTS_FILE, &v).await
}

pub async fn read_conversation(did: &str) -> Vec<DmMessage> {
    storage::read_json(&conv_path(did))
        .await
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

async fn write_conversation(did: &str, msgs: &[DmMessage]) -> Result<(), RuntimeError> {
    let v = serde_json::to_value(msgs).map_err(|e| RuntimeError::new(format!("serialize: {e}")))?;
    storage::write_json(&conv_path(did), &v).await
}

async fn upsert_contact(did: &str, last_preview: &str, ts: i64, inc_unread: u32) {
    let mut list = list_contacts().await;
    if let Some(c) = list.iter_mut().find(|c| c.did == did) {
        c.last_ts = ts;
        c.last_preview = last_preview.chars().take(140).collect();
        c.unread = c.unread.saturating_add(inc_unread);
    } else {
        list.push(DmContact {
            did: did.into(),
            name: String::new(),
            last_ts: ts,
            last_preview: last_preview.chars().take(140).collect(),
            unread: inc_unread,
        });
    }
    list.sort_by(|a, b| b.last_ts.cmp(&a.last_ts));
    let _ = write_contacts(&list).await;
}

pub async fn mark_read(did: &str) {
    let mut list = list_contacts().await;
    if let Some(c) = list.iter_mut().find(|c| c.did == did) {
        c.unread = 0;
        let _ = write_contacts(&list).await;
    }
}

pub async fn total_unread() -> u32 {
    list_contacts().await.iter().map(|c| c.unread).sum()
}

// ── peer key cache ─────────────────────────────────────────────────

async fn read_peer_keys() -> HashMap<String, PeerKeys> {
    storage::read_json(PEER_KEYS_FILE)
        .await
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

async fn write_peer_keys(map: &HashMap<String, PeerKeys>) -> Result<(), RuntimeError> {
    let v = serde_json::to_value(map).map_err(|e| RuntimeError::new(format!("serialize: {e}")))?;
    storage::write_json(PEER_KEYS_FILE, &v).await
}

pub async fn cache_peer_keys(did: &str, keys: PeerKeys) {
    let mut map = read_peer_keys().await;
    map.insert(did.into(), keys);
    let _ = write_peer_keys(&map).await;
}

pub async fn get_peer_keys(did: &str) -> Option<PeerKeys> {
    read_peer_keys().await.get(did).cloned()
}

// ── send / receive ─────────────────────────────────────────────────

// Reconstruct our private keys from the persisted session.
fn load_my_keys() -> Result<UserKeys, String> {
    let s = session::current().ok_or_else(|| "not signed in".to_string())?;
    let seed_vec = hex_to_bytes(&s.auth_key_hex)?;
    if seed_vec.len() != 32 {
        return Err("auth_key length mismatch".into());
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&seed_vec);
    let kem_secret = B64
        .decode(&s.ml_kem_secret_b64)
        .map_err(|e| format!("ml-kem secret b64: {e}"))?;
    let kem_public = B64
        .decode(&s.ml_kem_public_b64)
        .map_err(|e| format!("ml-kem public b64: {e}"))?;
    Ok(crypto::keys_from_seed_and_kem(&seed, &kem_secret, &kem_public))
}

fn my_public_pubkeys() -> Option<PeerKeys> {
    let s = session::current()?;
    if s.ml_kem_public_b64.is_empty() || s.auth_key_hex.is_empty() {
        return None;
    }
    let seed_vec = hex_to_bytes(&s.auth_key_hex).ok()?;
    if seed_vec.len() != 32 {
        return None;
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&seed_vec);
    let (_, x_pub) = crypto::x25519_from_seed(&seed);
    Some(PeerKeys {
        x25519_pub_b64: B64.encode(x_pub),
        ml_kem_pub_b64: s.ml_kem_public_b64,
    })
}

/// Send a message. If we have the peer's pubkeys cached, wraps the text
/// in a hybrid PQ envelope; otherwise sends plaintext (bootstrap so the
/// peer can cache our keys + reply encrypted).
pub async fn send_message(peer_did: &str, text: &str) -> Result<DmMessage, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("empty message".into());
    }
    let me = ensure_profile().await.map_err(|e| e.to_string())?;
    if peer_did == me.did_key {
        return Err("cannot DM yourself".into());
    }
    let s = session::current().ok_or_else(|| "not signed in".to_string())?;

    let plain_text: String = trimmed.chars().take(4096).collect();
    let my_pub = my_public_pubkeys();
    let peer_keys = get_peer_keys(peer_did).await;
    let encrypted = peer_keys.is_some();

    let msg = DmMessage {
        id: uuid::Uuid::new_v4().to_string(),
        text: plain_text.clone(),
        ts: now_ms(),
        mine: true,
        encrypted,
    };

    // 1. Local write — we always store the plaintext on our own side.
    let mut conv = read_conversation(peer_did).await;
    conv.push(msg.clone());
    write_conversation(peer_did, &conv)
        .await
        .map_err(|e| e.to_string())?;
    upsert_contact(peer_did, &msg.text, msg.ts, 0).await;

    // 2. Build the payload — encrypted envelope if we have pubkeys,
    //    plaintext bootstrap otherwise.
    let payload = if let Some(pk) = peer_keys {
        let recipient_x25519: [u8; 32] = B64
            .decode(&pk.x25519_pub_b64)
            .map_err(|e| format!("peer x25519 b64: {e}"))?
            .try_into()
            .map_err(|_| "peer x25519 wrong size".to_string())?;
        let recipient_kem = B64
            .decode(&pk.ml_kem_pub_b64)
            .map_err(|e| format!("peer ml-kem b64: {e}"))?;
        let env = crypto::encrypt_to_hybrid(&plain_text, &recipient_x25519, &recipient_kem)?;
        json!({
            "sender_pubkeys": my_pub,
            "envelope": env,
            "ts": msg.ts,
        })
    } else {
        json!({
            "sender_pubkeys": my_pub,
            "text": plain_text,
            "ts": msg.ts,
            "bootstrap": true,
        })
    };

    // 3. Sign + publish on the recipient's DM topic.
    let evt = create_signed_event("dm.message", payload, &s.auth_key_hex)?;
    let wire = crate::events::to_wire_string(&evt);
    let _ = peer::join_topic(&format!("hey-v0/dm/{peer_did}")).await;
    let _ = peer::publish(peer::PublishArgs {
        topic: &format!("hey-v0/dm/{peer_did}"),
        message: &wire,
        sender_id: &evt.sender_did,
        ts: evt.ts,
        signature: &evt.signature,
    })
    .await;
    Ok(msg)
}

/// Receive a message (called by peer_receiver). Caller has already
/// verified the Ed25519 signature against sender_did. Decrypts the
/// envelope if present, falls back to the plaintext bootstrap path.
/// Always caches the sender's pubkeys so future replies are encrypted.
pub async fn receive_message(sender_did: &str, payload: &Value) -> Result<(), String> {
    // 1. Cache sender's pubkeys if present.
    if let Some(pk) = payload.get("sender_pubkeys") {
        if let Ok(parsed) = serde_json::from_value::<PeerKeys>(pk.clone()) {
            cache_peer_keys(sender_did, parsed).await;
        }
    }

    // 2. Decrypt or bootstrap.
    let (text, encrypted) = if let Some(env_val) = payload.get("envelope") {
        let env: HpqEnvelope = serde_json::from_value(env_val.clone())
            .map_err(|e| format!("envelope shape: {e}"))?;
        let my_keys = load_my_keys()?;
        let pt = crypto::decrypt_hybrid(&env, &my_keys)?;
        (pt, true)
    } else if let Some(t) = payload.get("text").and_then(|v| v.as_str()) {
        (t.to_string(), false)
    } else {
        return Err("dm.message has neither envelope nor text".into());
    };

    let ts = payload
        .get("ts")
        .and_then(|v| v.as_i64())
        .unwrap_or_else(now_ms);

    let msg = DmMessage {
        id: uuid::Uuid::new_v4().to_string(),
        text: text.chars().take(4096).collect(),
        ts,
        mine: false,
        encrypted,
    };
    let mut conv = read_conversation(sender_did).await;
    conv.push(msg.clone());
    write_conversation(sender_did, &conv)
        .await
        .map_err(|e| e.to_string())?;
    upsert_contact(sender_did, &msg.text, msg.ts, 1).await;
    Ok(())
}
