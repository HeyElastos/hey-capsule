// Direct-message API with E2E hybrid post-quantum encryption.
//
// v2 (default for new contacts) — METADATA-SAFE per-pair queues.
//
//   Each contact has a private random 256-bit queue ID. The wire-level
//   topic is `hey-v0/q/<queue_id>` — the recipient's DID never appears
//   in the topic name, so the `peer` provider sees only opaque queue
//   traffic between random pseudonyms. Equivalent to SimpleX Chat's
//   unidirectional queue model adapted to Carrier gossipsub.
//
//   Sealed-sender envelope: every byte of {sender_did, signature, text}
//   lives INSIDE the ChaCha20-Poly1305 ciphertext. The provider sees
//   only `{ "type": "dm.v2", "envelope": HpqEnvelope }` — no DID, no
//   signature, no plaintext, no length-distinguishable metadata.
//
// v1 (legacy) — kept so existing contacts created before v2 still work.
//
//   Topic `hey-v0/dm/<recipient_did>` with the recipient's DID in the
//   path — leaks the social graph at the routing layer. We keep
//   receiving on this topic for back-compat, but new contacts always
//   use v2.
//
// Bootstrap problem solved: the FIRST message between strangers is
// negotiated via an OOB invite link, not a plaintext fallback. The link
// carries Alice's pubkeys + queue_id; Bob's reply carries his. No
// plaintext is ever sent over the wire.
//
// Storage:
//   Hey/dm/contacts.json      — [ Contact { did, queue stuff, ... } ]
//   Hey/dm/by-did/<did>.json  — [ { id, text, ts, mine, encrypted } ]
//   Hey/dm/expiry.json        — per-contact TTL
//   Hey/dm/peer-keys.json     — DEPRECATED (kept readable for migration)

use base64::engine::general_purpose::STANDARD as B64;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use base64::Engine as _;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::api::profile::ensure_profile;
use crate::crypto::{self, HpqEnvelope, UserKeys};
use crate::events::canonicalize;
use crate::identity::{
    bytes_to_hex, did_key_to_public_key, hex_to_bytes, sign, verify,
};
use crate::runtime::{peer, storage, RuntimeError};
use crate::session;

const CONTACTS_FILE: &str = "dm/contacts.json";
const PEER_KEYS_FILE: &str = "dm/peer-keys.json";
const EXPIRY_FILE: &str = "dm/expiry.json";

const TOPIC_PREFIX_V1: &str = "hey-v0/dm";
const TOPIC_PREFIX_V2: &str = "hey-v0/q";

const KIND_MESSAGE: &str = "message";
const KIND_HANDSHAKE: &str = "handshake";

/// Invite-link wire version. Bumping this invalidates old links so we
/// can safely change the embedded JSON shape.
const INVITE_LINK_VERSION: u8 = 1;

fn conv_path(did: &str) -> String {
    let safe = did.replace(['/', ':'], "_");
    format!("dm/by-did/{safe}.json")
}

fn now_ms() -> i64 {
    js_sys::Date::now() as i64
}

fn random_hex(n_bytes: usize) -> String {
    let mut buf = vec![0u8; n_bytes];
    OsRng.fill_bytes(&mut buf);
    bytes_to_hex(&buf)
}

// ── Contact ──────────────────────────────────────────────────────────
//
// Persisted in dm/contacts.json. A v2 contact has Some(queue stuff);
// a v1 (legacy) contact has None and the old hey-v0/dm/<did> path is
// used. Migration is incremental: we never auto-upgrade a v1 contact
// in place — the upgrade happens when the user generates a fresh invite.

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContactStatus {
    /// We minted an invite for this contact and are waiting for their
    /// handshake. Outgoing messages are queued (we don't know their
    /// queue/keys yet); UI shows "Awaiting reply…".
    PendingInvite,
    /// They sent a handshake; we have their queue + pubkeys; messages
    /// can flow in both directions.
    Active,
}

impl Default for ContactStatus {
    fn default() -> Self {
        ContactStatus::Active
    }
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

    // ── v2 fields. None ⇒ legacy v1 contact (route via hey-v0/dm/<did>).
    /// 256-bit random hex — topic we listen on for messages from this
    /// contact. We share this in our outbound invite.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub my_inbound_queue: Option<String>,
    /// 128-bit random hex — opaque consumer_id we present to the peer
    /// provider when reading from `my_inbound_queue`. Unlinkable to DID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub my_recv_pseudonym: Option<String>,
    /// 256-bit random hex — their queue (we publish here when sending
    /// to them). Filled in when their handshake arrives, or when WE
    /// accept their invite link.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub their_inbound_queue: Option<String>,
    /// 128-bit random hex — opaque sender_id we present to the peer
    /// provider when publishing to `their_inbound_queue`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub my_send_pseudonym: Option<String>,
    /// Their X25519 + ML-KEM pubkeys, cached at handshake time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_pubkeys: Option<PeerKeys>,
    /// Lifecycle flag. Default for legacy load is Active so existing
    /// contacts keep working.
    #[serde(default)]
    pub status: ContactStatus,
}

impl DmContact {
    /// True if this contact is fully wired up for v2 (we have their
    /// queue + pubkeys). False ⇒ either legacy v1 or pending invite.
    pub fn is_v2_active(&self) -> bool {
        self.peer_pubkeys.is_some()
            && self.their_inbound_queue.is_some()
            && self.my_inbound_queue.is_some()
    }

    /// True if this is a legacy contact created before per-pair queues.
    pub fn is_legacy(&self) -> bool {
        self.my_inbound_queue.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmMessage {
    pub id: String,
    pub text: String,
    pub ts: i64,
    pub mine: bool,
    /// True if this message was delivered through the E2E envelope path,
    /// false if it was a plaintext bootstrap (only possible for legacy
    /// v1 contacts; v2 sends are always encrypted).
    #[serde(default)]
    pub encrypted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerKeys {
    pub x25519_pub_b64: String,
    pub ml_kem_pub_b64: String,
}

// ── Contact list CRUD ────────────────────────────────────────────────

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

pub async fn find_contact(did: &str) -> Option<DmContact> {
    list_contacts().await.into_iter().find(|c| c.did == did)
}

/// Upsert one contact in the persisted list. Returns the resulting
/// (possibly-updated) record so callers can inspect queue/key state.
async fn upsert_contact_record(contact: DmContact) -> Result<DmContact, RuntimeError> {
    let mut list = list_contacts().await;
    let mut updated = contact;
    if let Some(pos) = list.iter().position(|c| c.did == updated.did) {
        // Preserve unread + ts from existing if the upsert doesn't
        // bring fresh ones (caller-controlled).
        let existing = &list[pos];
        if updated.last_ts == 0 {
            updated.last_ts = existing.last_ts;
        }
        if updated.last_preview.is_empty() {
            updated.last_preview = existing.last_preview.clone();
        }
        if updated.name.is_empty() {
            updated.name = existing.name.clone();
        }
        list[pos] = updated.clone();
    } else {
        list.push(updated.clone());
    }
    list.sort_by(|a, b| b.last_ts.cmp(&a.last_ts));
    write_contacts(&list).await?;
    Ok(updated)
}

async fn touch_contact_message(
    did: &str,
    preview: &str,
    ts: i64,
    inc_unread: u32,
) -> Result<(), RuntimeError> {
    let mut list = list_contacts().await;
    if let Some(c) = list.iter_mut().find(|c| c.did == did) {
        c.last_ts = ts;
        c.last_preview = preview.chars().take(140).collect();
        c.unread = c.unread.saturating_add(inc_unread);
    } else {
        // Legacy path: create a v1 contact on first sight.
        list.push(DmContact {
            did: did.into(),
            name: String::new(),
            last_ts: ts,
            last_preview: preview.chars().take(140).collect(),
            unread: inc_unread,
            my_inbound_queue: None,
            my_recv_pseudonym: None,
            their_inbound_queue: None,
            my_send_pseudonym: None,
            peer_pubkeys: None,
            status: ContactStatus::Active,
        });
    }
    list.sort_by(|a, b| b.last_ts.cmp(&a.last_ts));
    write_contacts(&list).await
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

// ── Expiry (per-contact TTL) ─────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ExpiryMap {
    #[serde(default)]
    map: HashMap<String, i64>,
}

async fn read_expiry() -> ExpiryMap {
    storage::read_json(EXPIRY_FILE)
        .await
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

async fn write_expiry(m: &ExpiryMap) -> Result<(), RuntimeError> {
    let v = serde_json::to_value(m).map_err(|e| RuntimeError::new(format!("serialize: {e}")))?;
    storage::write_json(EXPIRY_FILE, &v).await
}

pub async fn get_expiry_secs(did: &str) -> i64 {
    read_expiry().await.map.get(did).copied().unwrap_or(0)
}

pub async fn set_expiry_secs(did: &str, secs: i64) -> Result<(), RuntimeError> {
    let mut m = read_expiry().await;
    if secs <= 0 {
        m.map.remove(did);
    } else {
        m.map.insert(did.into(), secs);
    }
    write_expiry(&m).await
}

pub async fn prune_expired(did: &str) {
    let ttl = get_expiry_secs(did).await;
    if ttl <= 0 {
        return;
    }
    let cutoff = now_ms() - ttl * 1000;
    let conv = read_conversation(did).await;
    if conv.iter().any(|m| m.ts < cutoff) {
        let kept: Vec<DmMessage> = conv.into_iter().filter(|m| m.ts >= cutoff).collect();
        let _ = write_conversation(did, &kept).await;
    }
}

// ── Legacy peer-keys cache (read-only for migration) ────────────────

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

// ── Key material helpers ─────────────────────────────────────────────

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

// ── Invite link codec ────────────────────────────────────────────────
//
// An invite link is the OOB introduction. Alice generates one for each
// new contact, sends it through any channel (QR, email, Signal, IRL),
// and the recipient pastes it to bootstrap a metadata-safe DM channel.
//
// Link payload (base64url-encoded JSON, no padding):
//   {
//     "v":     1,
//     "queue": "<256bit hex>",      ← Alice's inbound queue
//     "did":   "did:key:z...",      ← Alice's identity (sig verification)
//     "name":  "Alice",
//     "keys":  { "x25519_pub_b64", "ml_kem_pub_b64" },
//     "nonce": "<128bit hex>"       ← per-link random, opaque
//   }
//
// The DID is in the link because (a) it's an OOB channel, by definition
// shared in confidence, and (b) the recipient needs it to verify the
// inner Ed25519 signature on Alice's first encrypted reply. The link is
// never sent over the runtime — once consumed, it's destroyed.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InviteLink {
    pub v: u8,
    pub queue: String,
    pub did: String,
    #[serde(default)]
    pub name: String,
    pub keys: PeerKeys,
    pub nonce: String,
}

pub fn encode_invite_link(invite: &InviteLink) -> String {
    let j = serde_json::to_vec(invite).unwrap_or_default();
    B64URL.encode(&j)
}

pub fn decode_invite_link(token: &str) -> Result<InviteLink, String> {
    // Tolerate users pasting "hey-invite:" prefix or whitespace/newlines.
    let stripped = token.trim();
    let stripped = stripped
        .strip_prefix("hey-invite:")
        .unwrap_or(stripped)
        .trim();
    let bytes = B64URL
        .decode(stripped)
        .map_err(|e| format!("invite base64: {e}"))?;
    let invite: InviteLink =
        serde_json::from_slice(&bytes).map_err(|e| format!("invite json: {e}"))?;
    if invite.v != INVITE_LINK_VERSION {
        return Err(format!(
            "unsupported invite link version {} (expected {INVITE_LINK_VERSION})",
            invite.v
        ));
    }
    if !invite.did.starts_with("did:key:z") {
        return Err("invite did is not a did:key".into());
    }
    if invite.queue.len() != 64 {
        return Err("invite queue is not 256-bit hex".into());
    }
    Ok(invite)
}

/// Mint a fresh invite for an unknown contact. The recipient's DID
/// isn't required at generation time — it's recovered from the inner
/// signature on their handshake reply. The contact is stashed under a
/// placeholder DID (`pending:<queue>`) until the handshake lands.
///
/// `display_label` is what we want to see in our own contact list for
/// this pending invite (e.g. "Bob from work"). Cosmetic; the real name
/// is overwritten by the handshake body if the peer sends one.
pub async fn generate_invite(display_label: &str) -> Result<String, String> {
    let me = ensure_profile().await.map_err(|e| e.to_string())?;
    let my_pub = my_public_pubkeys().ok_or_else(|| "no pubkeys (not signed in)".to_string())?;

    let queue = random_hex(32);
    let recv_pseudonym = random_hex(16);
    let send_pseudonym = random_hex(16);
    let nonce = random_hex(16);

    // Placeholder DID until the handshake reply arrives and gives us
    // the real one. We disambiguate pending invites by queue id.
    let placeholder_did = format!("pending:{queue}");
    let contact = DmContact {
        did: placeholder_did,
        name: display_label.trim().to_string(),
        last_ts: now_ms(),
        last_preview: String::from("Invite sent — awaiting reply"),
        unread: 0,
        my_inbound_queue: Some(queue.clone()),
        my_recv_pseudonym: Some(recv_pseudonym),
        their_inbound_queue: None,
        my_send_pseudonym: Some(send_pseudonym),
        peer_pubkeys: None,
        status: ContactStatus::PendingInvite,
    };
    upsert_contact_record(contact)
        .await
        .map_err(|e| e.to_string())?;

    // Join our new inbound queue topic so the peer_receiver picks up
    // their handshake reply.
    let _ = peer::join_topic(&format!("{TOPIC_PREFIX_V2}/{queue}")).await;

    let invite = InviteLink {
        v: INVITE_LINK_VERSION,
        queue,
        did: me.did_key.clone(),
        name: me.name.clone(),
        keys: my_pub,
        nonce,
    };
    Ok(format!("hey-invite:{}", encode_invite_link(&invite)))
}

/// Accept someone else's invite link. Creates an Active contact, sends
/// the handshake reply (encrypted to their pubkeys) to their queue, and
/// returns the contact's DID so the UI can navigate to the conversation.
pub async fn accept_invite(token: &str) -> Result<String, String> {
    let invite = decode_invite_link(token)?;
    let me = ensure_profile().await.map_err(|e| e.to_string())?;
    if invite.did == me.did_key {
        return Err("that's your own invite link".into());
    }
    let s = session::current().ok_or_else(|| "not signed in".to_string())?;
    let my_pub = my_public_pubkeys().ok_or_else(|| "no pubkeys (not signed in)".to_string())?;

    // Mint OUR queue for receiving from them.
    let my_queue = random_hex(32);
    let my_recv_pseudonym = random_hex(16);
    let my_send_pseudonym = random_hex(16);

    let contact = DmContact {
        did: invite.did.clone(),
        name: invite.name.clone(),
        last_ts: now_ms(),
        last_preview: String::from("Invite accepted"),
        unread: 0,
        my_inbound_queue: Some(my_queue.clone()),
        my_recv_pseudonym: Some(my_recv_pseudonym),
        their_inbound_queue: Some(invite.queue.clone()),
        my_send_pseudonym: Some(my_send_pseudonym.clone()),
        peer_pubkeys: Some(invite.keys.clone()),
        status: ContactStatus::Active,
    };
    let _ = upsert_contact_record(contact)
        .await
        .map_err(|e| e.to_string())?;

    // Join our new inbound queue so the receiver picks up their replies.
    let _ = peer::join_topic(&format!("{TOPIC_PREFIX_V2}/{my_queue}")).await;

    // Build & send the handshake reply on THEIR queue.
    let handshake_body = json!({
        "my_inbound_queue": my_queue,
        "name": me.name.clone(),
        "pubkeys": my_pub,
    });

    let inner = build_inner(KIND_HANDSHAKE, &handshake_body, &me.did_key, &s.auth_key_hex)?;
    let envelope = encrypt_inner_for_peer(&inner, &invite.keys)?;
    let wire = json!({
        "type": "dm.v2",
        "envelope": envelope,
    })
    .to_string();

    let _ = peer::join_topic(&format!("{TOPIC_PREFIX_V2}/{}", invite.queue)).await;
    let _ = peer::publish(peer::PublishArgs {
        topic: &format!("{TOPIC_PREFIX_V2}/{}", invite.queue),
        message: &wire,
        // Sealed-sender at the provider layer: random pseudonym, not DID.
        sender_id: &my_send_pseudonym,
        ts: now_ms(),
        // Provider-layer signature isn't used by the recv side (we verify
        // the inner sig after decrypt). Send empty to avoid leaking
        // anything bound to our identity.
        signature: "",
    })
    .await;

    Ok(invite.did)
}

// ── Sealed-sender envelope plumbing ──────────────────────────────────
//
// Inner payload — what lives inside the ChaCha20-Poly1305 ciphertext.
// The provider never sees this.

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InnerPayload {
    kind: String,
    sender_did: String,
    ts: i64,
    body: Value,
    /// Ed25519 sig over `canonicalize({kind, body, sender_did, ts})`.
    sig: String,
}

fn build_inner(
    kind: &str,
    body: &Value,
    sender_did: &str,
    auth_key_hex: &str,
) -> Result<InnerPayload, String> {
    let ts = now_ms();
    let to_sign = canonicalize(&json!({
        "kind": kind,
        "body": body,
        "sender_did": sender_did,
        "ts": ts,
    }));
    let seed_vec = hex_to_bytes(auth_key_hex)?;
    if seed_vec.len() != 32 {
        return Err("seed length".into());
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&seed_vec);
    let sig = sign(to_sign.as_bytes(), &seed);
    Ok(InnerPayload {
        kind: kind.into(),
        sender_did: sender_did.into(),
        ts,
        body: body.clone(),
        sig,
    })
}

fn verify_inner(inner: &InnerPayload) -> bool {
    if !inner.sender_did.starts_with("did:key:z") {
        return false;
    }
    let pk = match did_key_to_public_key(&inner.sender_did) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let to_sign = canonicalize(&json!({
        "kind": inner.kind,
        "body": inner.body,
        "sender_did": inner.sender_did,
        "ts": inner.ts,
    }));
    verify(to_sign.as_bytes(), &inner.sig, &pk)
}

fn encrypt_inner_for_peer(
    inner: &InnerPayload,
    peer_keys: &PeerKeys,
) -> Result<HpqEnvelope, String> {
    let plaintext = serde_json::to_string(inner).map_err(|e| format!("inner json: {e}"))?;
    let recipient_x25519: [u8; 32] = B64
        .decode(&peer_keys.x25519_pub_b64)
        .map_err(|e| format!("peer x25519 b64: {e}"))?
        .try_into()
        .map_err(|_| "peer x25519 wrong size".to_string())?;
    let recipient_kem = B64
        .decode(&peer_keys.ml_kem_pub_b64)
        .map_err(|e| format!("peer ml-kem b64: {e}"))?;
    crypto::encrypt_to_hybrid(&plaintext, &recipient_x25519, &recipient_kem)
}

fn decrypt_envelope_to_inner(env: &HpqEnvelope) -> Result<InnerPayload, String> {
    let keys = load_my_keys()?;
    let pt = crypto::decrypt_hybrid(env, &keys)?;
    serde_json::from_str(&pt).map_err(|e| format!("inner deserialize: {e}"))
}

// ── Public send / receive entry points ───────────────────────────────

/// Send a message. v2 path (sealed sender, per-pair queue) is used when
/// the contact is is_v2_active(); otherwise we fall through to the
/// legacy v1 path for back-compat with contacts created before queues.
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
    let contact = find_contact(peer_did).await;

    // PendingInvite — they haven't replied to our invite yet.
    if let Some(c) = &contact {
        if c.status == ContactStatus::PendingInvite {
            return Err("Awaiting their invite acceptance — they haven't replied yet.".into());
        }
    }

    let use_v2 = contact.as_ref().map(|c| c.is_v2_active()).unwrap_or(false);

    // Local-side message (mine=true), always plaintext on disk. The
    // `encrypted` flag is for our own UI hint; v2 path is always
    // encrypted; legacy v1 is encrypted iff we've cached peer keys.
    let legacy_encrypted = !use_v2 && get_peer_keys(peer_did).await.is_some();
    let msg = DmMessage {
        id: uuid::Uuid::new_v4().to_string(),
        text: plain_text.clone(),
        ts: now_ms(),
        mine: true,
        encrypted: use_v2 || legacy_encrypted,
    };
    let mut conv = read_conversation(peer_did).await;
    conv.push(msg.clone());
    write_conversation(peer_did, &conv)
        .await
        .map_err(|e| e.to_string())?;
    touch_contact_message(peer_did, &msg.text, msg.ts, 0)
        .await
        .map_err(|e| e.to_string())?;

    if use_v2 {
        let c = contact.unwrap();
        let queue = c.their_inbound_queue.as_deref().unwrap();
        let send_pseudonym = c.my_send_pseudonym.as_deref().unwrap_or("anonymous");
        let peer_keys = c.peer_pubkeys.as_ref().unwrap();

        let body = json!({ "text": plain_text });
        let inner = build_inner(KIND_MESSAGE, &body, &me.did_key, &s.auth_key_hex)?;
        let envelope = encrypt_inner_for_peer(&inner, peer_keys)?;
        let wire = json!({
            "type": "dm.v2",
            "envelope": envelope,
        })
        .to_string();
        let _ = peer::join_topic(&format!("{TOPIC_PREFIX_V2}/{queue}")).await;
        let _ = peer::publish(peer::PublishArgs {
            topic: &format!("{TOPIC_PREFIX_V2}/{queue}"),
            message: &wire,
            sender_id: send_pseudonym,
            ts: msg.ts,
            signature: "",
        })
        .await;
        return Ok(msg);
    }

    // ── Legacy v1 path (kept for existing contacts) ──────────────────

    let my_pub = my_public_pubkeys();
    let peer_keys = get_peer_keys(peer_did).await;
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

    let evt = crate::events::create_signed_event("dm.message", payload, &s.auth_key_hex)?;
    let wire = crate::events::to_wire_string(&evt);
    let _ = peer::join_topic(&format!("{TOPIC_PREFIX_V1}/{peer_did}")).await;
    let _ = peer::publish(peer::PublishArgs {
        topic: &format!("{TOPIC_PREFIX_V1}/{peer_did}"),
        message: &wire,
        sender_id: &evt.sender_did,
        ts: evt.ts,
        signature: &evt.signature,
    })
    .await;
    Ok(msg)
}

/// Receive a v1 (legacy) DM. Called by peer_receiver for the deprecated
/// `hey-v0/dm/<my_did>` topic. Same shape as before — we keep it so
/// contacts who haven't migrated to v2 still reach us.
pub async fn receive_message(sender_did: &str, payload: &Value) -> Result<(), String> {
    if let Some(pk) = payload.get("sender_pubkeys") {
        if let Ok(parsed) = serde_json::from_value::<PeerKeys>(pk.clone()) {
            cache_peer_keys(sender_did, parsed).await;
        }
    }
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
    touch_contact_message(sender_did, &msg.text, msg.ts, 1)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Extract the queue id from a `hey-v0/q/<id>` topic. Returns None if
/// the topic doesn't match the expected shape.
fn queue_id_from_topic(topic: &str) -> Option<&str> {
    topic.strip_prefix(&format!("{TOPIC_PREFIX_V2}/"))
}

/// Receive a v2 (sealed-sender) DM from a per-pair queue topic. Called
/// by peer_receiver when it pulls a wire entry from `hey-v0/q/<id>`.
///
/// The provider has handed us the wire string (`{ type: "dm.v2",
/// envelope }`) and the topic it came from. We decrypt the envelope,
/// verify the inner signature, and dispatch on inner.kind. For
/// handshakes we resolve the pending contact by the queue id (since
/// the sender's real DID was previously unknown to us).
pub async fn receive_v2_wire(topic: &str, wire: &str) -> Result<(), String> {
    let v: Value = serde_json::from_str(wire).map_err(|e| format!("wire json: {e}"))?;
    if v.get("type").and_then(|t| t.as_str()) != Some("dm.v2") {
        return Err("not a dm.v2 wire".into());
    }
    let env_val = v.get("envelope").ok_or_else(|| "no envelope".to_string())?;
    let envelope: HpqEnvelope =
        serde_json::from_value(env_val.clone()).map_err(|e| format!("envelope shape: {e}"))?;
    let inner = decrypt_envelope_to_inner(&envelope)?;
    if !verify_inner(&inner) {
        return Err("inner signature mismatch".into());
    }
    match inner.kind.as_str() {
        KIND_MESSAGE => {
            let text = inner
                .body
                .get("text")
                .and_then(|t| t.as_str())
                .ok_or_else(|| "message body has no text".to_string())?;
            // Defense in depth: drop if the sender_did doesn't match a
            // known active v2 contact whose `my_inbound_queue` equals
            // this topic. Stops a stranger from delivering messages
            // via a leaked queue id.
            let queue_id = queue_id_from_topic(topic).ok_or_else(|| "bad topic".to_string())?;
            let owns_queue = list_contacts()
                .await
                .into_iter()
                .any(|c| {
                    c.did == inner.sender_did
                        && c.my_inbound_queue.as_deref() == Some(queue_id)
                        && c.status == ContactStatus::Active
                });
            if !owns_queue {
                return Err("sender does not match queue owner".into());
            }
            let msg = DmMessage {
                id: uuid::Uuid::new_v4().to_string(),
                text: text.chars().take(4096).collect(),
                ts: inner.ts,
                mine: false,
                encrypted: true,
            };
            let mut conv = read_conversation(&inner.sender_did).await;
            conv.push(msg.clone());
            write_conversation(&inner.sender_did, &conv)
                .await
                .map_err(|e| e.to_string())?;
            touch_contact_message(&inner.sender_did, &msg.text, msg.ts, 1)
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        }
        KIND_HANDSHAKE => {
            let queue_id = queue_id_from_topic(topic).ok_or_else(|| "bad topic".to_string())?;
            receive_handshake(&inner, queue_id).await
        }
        other => Err(format!("unknown inner kind: {other}")),
    }
}

/// Handle a handshake reply that landed on one of OUR queues. The
/// queue id (NOT the sender_did) is the disambiguator — when we
/// minted the invite we didn't know who the recipient would be.
async fn receive_handshake(inner: &InnerPayload, on_queue: &str) -> Result<(), String> {
    let their_queue = inner
        .body
        .get("my_inbound_queue")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "handshake missing my_inbound_queue".to_string())?;
    let their_keys: PeerKeys = inner
        .body
        .get("pubkeys")
        .ok_or_else(|| "handshake missing pubkeys".to_string())
        .and_then(|v| serde_json::from_value(v.clone()).map_err(|e| format!("pubkeys: {e}")))?;
    let their_name = inner
        .body
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    let mut list = list_contacts().await;
    let pos = list.iter().position(|c| {
        c.my_inbound_queue.as_deref() == Some(on_queue)
            && c.status == ContactStatus::PendingInvite
    });
    let Some(pos) = pos else {
        // No matching pending invite — either replayed handshake or a
        // stranger guessed the queue id (astronomically unlikely with
        // 256 bits of entropy). Drop silently.
        return Ok(());
    };

    let mut c = list.remove(pos);
    c.did = inner.sender_did.clone();
    c.their_inbound_queue = Some(their_queue.to_string());
    c.peer_pubkeys = Some(their_keys);
    c.status = ContactStatus::Active;
    if c.name.is_empty() || c.name.starts_with("pending:") {
        c.name = their_name.into();
    }
    c.last_ts = inner.ts;
    c.last_preview = "Invite accepted ✓".into();
    list.push(c);
    list.sort_by(|a, b| b.last_ts.cmp(&a.last_ts));
    write_contacts(&list).await.map_err(|e| e.to_string())?;
    Ok(())
}

// ── Helpers exposed to peer_receiver for subscription bookkeeping ────

/// Iterate v2 contacts and return the list of `hey-v0/q/<id>` topics we
/// must keep joined to receive their messages.
pub async fn my_v2_topics() -> Vec<(String, String)> {
    list_contacts()
        .await
        .into_iter()
        .filter_map(|c| {
            let q = c.my_inbound_queue?;
            let consumer_id = c
                .my_recv_pseudonym
                .unwrap_or_else(|| "anonymous".into());
            Some((format!("{TOPIC_PREFIX_V2}/{q}"), consumer_id))
        })
        .collect()
}

