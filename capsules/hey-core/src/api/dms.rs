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
/// v2 queues used to be `hey-v0/q/<rnd>`. We dropped the `hey-v0/`
/// prefix so an observer of the peer provider can't pick Hey-app
/// traffic out of arbitrary queue traffic by topic-name shape. Random
/// 256-bit ids still need a routing prefix; one ASCII char is enough.
const TOPIC_PREFIX_V2: &str = "q";

const KIND_MESSAGE: &str = "message";
const KIND_HANDSHAKE: &str = "handshake";
/// Sent by Alice on Bob's queue right after she processes his
/// handshake. Carries a fresh Alice-side queue id; lets Alice retire
/// the original invite queue so a leaked link can't be reused.
const KIND_WELCOME: &str = "welcome";

/// Invite-link wire version. Bumping this invalidates old links so we
/// can safely change the embedded JSON shape.
const INVITE_LINK_VERSION: u8 = 2;
/// How long an invite link is valid for, in ms. Pasting after this
/// expires fails with a clear error. 24 hours felt like the right
/// trade-off between "share now, accept later" and the leak window.
const INVITE_TTL_MS: i64 = 24 * 60 * 60 * 1000;

// ── Double Ratchet (M6) ──────────────────────────────────────────────
//
/// Per-contact ratchet state lives in its OWN file under this dir, NOT on
/// DmContact — so the (potentially large) skipped-keys blob never rides the
/// whole-contacts.json rewrite that runs on EVERY message (must-fix #7).
const RATCHET_DIR: &str = "dm/ratchet";
/// Max messages we will skip (and derive keys for) in a SINGLE chain advance.
/// A cleartext header claiming a jump larger than this is rejected BEFORE any
/// KDF runs, so a forged counter can't make us burn unbounded CPU (must-fix #7).
const MAX_SKIP: u32 = 1000;
/// Hard cap on stored out-of-order keys (FIFO eviction). Bounds memory.
const MAX_SKIPPED_KEYS: usize = 2000;
/// Skipped keys older than this are evicted — a message that never arrived.
const SKIPPED_TTL_MS: i64 = 7 * 24 * 60 * 60 * 1000;

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

/// Which identity OUR side of a conversation presents to the peer.
///
/// SimpleX-style "incognito": Regular uses the stable, federated did:key
/// from the session (cross-app, verifiable — the default); Anonymous uses
/// a per-contact ephemeral identity that is never linked to the real DID.
/// The mode only changes WHICH key signs the inner payload and WHICH
/// pubkeys/DID/name we advertise — the sealed-sender envelope (crypto.rs)
/// already carries nothing about the sender, so this is sufficient for
/// identity anonymity. It does NOT hide network metadata (node id / IP
/// still traverse Carrier — that needs the garlic overlay).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityMode {
    /// Stable did:key from the session. Default for every existing contact.
    #[default]
    Regular,
    /// Fresh per-contact Ed25519 + X25519 + ML-KEM identity, unlinkable to
    /// the real DID and to our other anonymous contacts.
    Anonymous,
}

/// A per-contact ephemeral identity used in Anonymous mode. Minted fresh
/// for ONE contact (never reused — that is what makes our anonymous
/// contacts mutually unlinkable) and never derived from the session
/// identity. Persisted locally on the contact; only its PUBLIC projection
/// (did + pubkeys) is ever put on the wire, in the invite/handshake.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnonIdentity {
    /// 32-byte Ed25519 seed (hex) — yields both the ephemeral signing key
    /// and (via x25519_from_seed) the ephemeral X25519 key.
    pub seed_hex: String,
    /// Ephemeral ML-KEM-768 secret (base64) — decrypts traffic the peer
    /// sealed to our advertised ephemeral pubkey.
    pub ml_kem_secret_b64: String,
    /// Ephemeral ML-KEM-768 public (base64) — advertised in the invite /
    /// handshake so the peer encrypts to this key, not our real one.
    pub ml_kem_public_b64: String,
    /// The ephemeral did:key (derived from seed_hex), cached so we present
    /// a stable pseudonym to this one contact without re-deriving each send.
    pub did: String,
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

    /// Identity OUR side presents to this contact. Defaults to Regular for
    /// every contact created before Anonymous mode (no field in old JSON).
    #[serde(default)]
    pub mode: IdentityMode,
    /// The ephemeral identity backing `mode == Anonymous`. None ⇒ Regular.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anon_identity: Option<AnonIdentity>,

    /// STICKY: true once a Double Ratchet was bootstrapped with this contact
    /// (both sides advertised ratchet support in the invite + handshake). Set
    /// ONCE at bootstrap and NEVER cleared — so a contact can't be silently
    /// downgraded back to the no-PCS single-shot path (must-fix #6). The
    /// ratchet STATE itself lives in dm/ratchet/<did>.json, not here.
    #[serde(default)]
    pub ratchet_capable: bool,
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
    /// E2E attachments (files/photos). Only the ciphertext lives in the blob
    /// store; the per-file key rides INSIDE this message's sealed payload, so
    /// the store/relay never sees plaintext. Fetched + decrypted on render.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
}

/// Reference to one end-to-end-encrypted attachment. The bytes are NOT stored
/// here — `cid` points at the ciphertext in the content store and `key_b64`
/// (carried only inside the sealed message) decrypts it. Plaintext `name`/`mime`
/// /`size` are sealed too (never on the wire in clear).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
    /// Content-store ref to the CIPHERTEXT (IPFS CID today; an iroh-blobs ticket
    /// once that backend is registered — the upload/fetch boundary is abstracted).
    pub cid: String,
    /// Base64 ChaCha20-Poly1305 key for this one file. Sealed E2E with the msg.
    pub key_b64: String,
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
            mode: IdentityMode::Regular,
            anon_identity: None,
            ratchet_capable: false,
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

/// Our advertised pubkeys, provider-aware: a provider-backed session (empty
/// local seed) gets them from the identity provider; otherwise they are
/// derived from the local seed. Used when minting invites/handshakes.
async fn my_pubkeys() -> Option<PeerKeys> {
    let s = session::current()?;
    if s.auth_key_hex.is_empty() {
        let resp = crate::runtime::identity_provider::pubkeys(IDENTITY_NS)
            .await
            .ok()?;
        let d = resp.get("data").unwrap_or(&resp);
        Some(PeerKeys {
            x25519_pub_b64: d.get("x25519_pub_b64")?.as_str()?.to_string(),
            ml_kem_pub_b64: d.get("ml_kem_pub_b64")?.as_str()?.to_string(),
        })
    } else {
        my_public_pubkeys()
    }
}

/// Adopt the runtime-projected identity with NO passkey tap — the wallet
/// model. Calls identity/whoami; on success installs a PROVIDER-BACKED session
/// (real did:key, EMPTY local seed → every signing + decryption routes through
/// the runtime identity provider). Returns the did, or None if the provider
/// isn't available (the caller then falls back to the passkey ceremony, so
/// removing the fork patch still leaves a working app).
pub async fn adopt_provider_identity() -> Option<String> {
    let resp = crate::runtime::identity_provider::whoami(IDENTITY_NS)
        .await
        .ok()?;
    let d = resp.get("data").unwrap_or(&resp);
    let did = d.get("did_key")?.as_str()?.to_string();
    if !did.starts_with("did:key:z") {
        return None;
    }
    let name = session::current()
        .map(|s| s.name)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| short_did_label(&did));
    session::set(&session::Session {
        auth_key_hex: String::new(),
        did_key: did.clone(),
        name,
        ml_kem_secret_b64: String::new(),
        ml_kem_public_b64: String::new(),
    });
    Some(did)
}

fn short_did_label(did: &str) -> String {
    if did.len() > 12 {
        format!("hey-{}", &did[did.len() - 6..])
    } else {
        did.to_string()
    }
}

// ── Per-contact identity (Regular vs Anonymous) ──────────────────────
//
// In Anonymous mode every outgoing artifact we put on the wire for a
// contact — the invite/handshake `did` + `pubkeys` + `name`, and the
// inner-payload `sender_did` + signature — comes from a fresh ephemeral
// identity instead of the session. The sealed-sender envelope already
// carries no sender key (see crypto::encrypt_to_hybrid), so swapping the
// signing key + advertised pubkeys is all it takes to make us unlinkable.

/// Parse a 64-hex-char string into a 32-byte seed.
fn seed32(hex: &str) -> Result<[u8; 32], String> {
    let v = hex_to_bytes(hex)?;
    if v.len() != 32 {
        return Err("seed must be 32 bytes".into());
    }
    let mut s = [0u8; 32];
    s.copy_from_slice(&v);
    Ok(s)
}

/// Mint a fresh ephemeral identity for one Anonymous contact: a random
/// Ed25519 seed (→ signing key + did:key + X25519) plus a fresh
/// ML-KEM-768 keypair. Never reused across contacts — that independence
/// is what keeps our anonymous contacts mutually unlinkable.
fn mint_anon_identity() -> Result<AnonIdentity, String> {
    let mut seed = [0u8; 32];
    OsRng.fill_bytes(&mut seed);
    let (kem_secret, kem_public) = crypto::generate_ml_kem_keypair();
    let kp = ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new(seed));
    let pk_bytes: [u8; 32] = *kp.pk;
    Ok(AnonIdentity {
        seed_hex: bytes_to_hex(&seed),
        ml_kem_secret_b64: B64.encode(&kem_secret),
        ml_kem_public_b64: B64.encode(&kem_public),
        did: crate::identity::public_key_to_did_key(&pk_bytes),
    })
}

/// Public X25519 + ML-KEM projection of an ephemeral identity — what we
/// advertise so the peer can encrypt to us in Anonymous mode.
fn anon_pubkeys(a: &AnonIdentity) -> Result<PeerKeys, String> {
    let seed = seed32(&a.seed_hex)?;
    let (_, x_pub) = crypto::x25519_from_seed(&seed);
    Ok(PeerKeys {
        x25519_pub_b64: B64.encode(x_pub),
        ml_kem_pub_b64: a.ml_kem_public_b64.clone(),
    })
}

/// Full key bundle for an ephemeral identity — used to DECRYPT traffic a
/// peer sealed to our advertised ephemeral pubkey.
fn anon_user_keys(a: &AnonIdentity) -> Result<UserKeys, String> {
    let seed = seed32(&a.seed_hex)?;
    let kem_secret = B64
        .decode(&a.ml_kem_secret_b64)
        .map_err(|e| format!("anon kem secret b64: {e}"))?;
    let kem_public = B64
        .decode(&a.ml_kem_public_b64)
        .map_err(|e| format!("anon kem public b64: {e}"))?;
    Ok(crypto::keys_from_seed_and_kem(&seed, &kem_secret, &kem_public))
}

/// The (did, signing-seed-hex) we present to a contact: the session
/// identity in Regular mode, the ephemeral identity in Anonymous mode.
fn signing_identity(
    mode: IdentityMode,
    anon: Option<&AnonIdentity>,
    me_did: &str,
    me_auth_key_hex: &str,
) -> Result<(String, String), String> {
    match mode {
        IdentityMode::Regular => Ok((me_did.to_string(), me_auth_key_hex.to_string())),
        IdentityMode::Anonymous => {
            let a = anon
                .ok_or_else(|| "anonymous contact is missing its ephemeral identity".to_string())?;
            Ok((a.did.clone(), a.seed_hex.clone()))
        }
    }
}

/// The pubkeys we advertise to a contact (real session pubkeys in Regular,
/// ephemeral pubkeys in Anonymous).
fn advertised_pubkeys(
    mode: IdentityMode,
    anon: Option<&AnonIdentity>,
    me_pub: &PeerKeys,
) -> Result<PeerKeys, String> {
    match mode {
        IdentityMode::Regular => Ok(me_pub.clone()),
        IdentityMode::Anonymous => {
            let a = anon
                .ok_or_else(|| "anonymous contact is missing its ephemeral identity".to_string())?;
            anon_pubkeys(a)
        }
    }
}

/// The display name we SHARE with a contact: our real profile name in
/// Regular mode, nothing in Anonymous mode (sharing it would defeat the
/// anonymity — the peer would learn who we are).
fn shared_display_name(mode: IdentityMode, real_name: &str) -> String {
    match mode {
        IdentityMode::Regular => real_name.to_string(),
        IdentityMode::Anonymous => String::new(),
    }
}

/// How to open incoming traffic: with local key material (the session seed,
/// or a per-contact anonymous ephemeral key), or via the runtime identity
/// provider (a provider-backed session has a did:key but no local seed).
enum DecryptVia {
    Local(UserKeys),
    Provider,
}

/// The decrypt path for a specific contact. Anonymous contacts ALWAYS decrypt
/// locally with their per-contact ephemeral key — never the provider, which
/// does not hold it (must-fix #3). For the regular identity: a provider-backed
/// session (empty seed) decrypts via the runtime; otherwise the local seed.
fn decrypt_via_for_contact(c: &DmContact) -> Result<DecryptVia, String> {
    if c.mode == IdentityMode::Anonymous {
        let a = c.anon_identity.as_ref().ok_or_else(|| {
            "anonymous contact is missing its ephemeral identity (decrypt)".to_string()
        })?;
        return Ok(DecryptVia::Local(anon_user_keys(a)?));
    }
    decrypt_via_for_session()
}

/// Decrypt path for the session identity itself (no per-contact override).
fn decrypt_via_for_session() -> Result<DecryptVia, String> {
    match session::current() {
        Some(s) if s.auth_key_hex.is_empty() => Ok(DecryptVia::Provider),
        _ => Ok(DecryptVia::Local(load_my_keys()?)),
    }
}

/// Choose the decrypt path for traffic arriving on `queue_id`. An unknown queue
/// (self-test/legacy) falls back to the session's path.
async fn decrypt_via_for_queue(queue_id: Option<&str>) -> Result<DecryptVia, String> {
    if let Some(qid) = queue_id {
        if let Some(c) = list_contacts()
            .await
            .into_iter()
            .find(|c| c.my_inbound_queue.as_deref() == Some(qid))
        {
            return decrypt_via_for_contact(&c);
        }
    }
    decrypt_via_for_session()
}

/// Compute the two hybrid shared secrets (X25519 ECDH output, ML-KEM
/// decapsulated secret) for an `(eph_pub, kem_ct)` pair — locally from our key
/// material, or via the identity provider (private keys never leave it). This
/// is the recipient half of both the single-shot decrypt AND the ratchet
/// bootstrap's SK recovery.
async fn shared_secrets(
    via: &DecryptVia,
    eph_pub: &[u8],
    kem_ct: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), String> {
    match via {
        DecryptVia::Local(keys) => {
            let eph: [u8; 32] = eph_pub
                .try_into()
                .map_err(|_| "eph wrong size".to_string())?;
            let x = crypto::dh(&keys.x25519_priv, &eph);
            let k = crypto::ml_kem_decapsulate_local(kem_ct, &keys.ml_kem_secret_bytes)?;
            Ok((x.to_vec(), k))
        }
        DecryptVia::Provider => {
            let x = crate::runtime::identity_provider::x25519_dh(IDENTITY_NS, eph_pub)
                .await
                .map_err(|e| format!("provider x25519_dh: {e}"))?;
            let k = crate::runtime::identity_provider::ml_kem_decapsulate(IDENTITY_NS, kem_ct)
                .await
                .map_err(|e| format!("provider ml_kem_decapsulate: {e}"))?;
            let x_shared =
                crate::runtime::identity_provider::shared_from(&x).map_err(|e| e.to_string())?;
            let k_shared =
                crate::runtime::identity_provider::shared_from(&k).map_err(|e| e.to_string())?;
            Ok((x_shared, k_shared))
        }
    }
}

/// Open one single-shot sealed envelope to plaintext (X25519-static + ML-KEM
/// hybrid). Ratchet messages instead supply the X25519-half as the chain
/// message key and only need the KEM-half (`ratchet_kem_ss`).
async fn open_envelope(env: &HpqEnvelope, via: &DecryptVia) -> Result<String, String> {
    let (eph, kem_ct) = crypto::envelope_recipient_inputs(env)?;
    let (x, k) = shared_secrets(via, &eph, &kem_ct).await?;
    crypto::open_with_secrets(env, &x, &k)
}

/// The ML-KEM shared secret for a ratchet envelope (its KEM-half). The X25519
/// half of a ratchet message is the chain message key `mk`, NOT an ECDH against
/// a static key, so we only decapsulate `env.kem` here. Anon ⇒ local anon key;
/// provider-backed ⇒ runtime; else local seed.
async fn ratchet_kem_ss(env: &HpqEnvelope, via: &DecryptVia) -> Result<Vec<u8>, String> {
    let (_eph, kem_ct) = crypto::envelope_recipient_inputs(env)?;
    match via {
        DecryptVia::Local(keys) => crypto::ml_kem_decapsulate_local(&kem_ct, &keys.ml_kem_secret_bytes),
        DecryptVia::Provider => {
            let k = crate::runtime::identity_provider::ml_kem_decapsulate(IDENTITY_NS, &kem_ct)
                .await
                .map_err(|e| format!("provider ml_kem_decapsulate: {e}"))?;
            crate::runtime::identity_provider::shared_from(&k).map_err(|e| e.to_string())
        }
    }
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
    /// Unix-ms expiry. `decode_invite_link` refuses tokens past this.
    /// Older v1 links omit it; for v=1 we treat as "no expiry."
    #[serde(default)]
    pub expires_at: i64,
    /// The inviter's ratchet prekey (Double Ratchet bootstrap). Additive +
    /// optional: an invite WITHOUT it (old link, or a peer that doesn't
    /// ratchet) negotiates the single-shot path. Present ⇒ the accepter can
    /// bootstrap a ratchet and signals it back in the handshake.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ratchet: Option<RatchetPrekey>,
}

pub fn encode_invite_link(invite: &InviteLink) -> String {
    let j = serde_json::to_vec(invite).unwrap_or_default();
    B64URL.encode(&j)
}

/// Render an invite link as a scannable QR-code SVG string. Used by
/// the chat UI to offer a "show QR" alternative to copy-paste. Returns
/// None if the link is too long for a QR code (very unlikely; v2 links
/// fit comfortably in version 27 ≈ 1500 bytes).
pub fn invite_qr_svg(token: &str) -> Option<String> {
    use qrcode::render::svg;
    use qrcode::{EcLevel, QrCode};
    let code = QrCode::with_error_correction_level(token.as_bytes(), EcLevel::M).ok()?;
    Some(
        code.render::<svg::Color<'_>>()
            .min_dimensions(220, 220)
            .dark_color(svg::Color("#0a0a0a"))
            .light_color(svg::Color("#ffffff"))
            .build(),
    )
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
    // We currently emit v=2 (with expires_at). Accept v=1 too so old
    // links keep working — they don't have expiry but every other
    // field is identical.
    if invite.v != INVITE_LINK_VERSION && invite.v != 1 {
        return Err(format!(
            "unsupported invite link version {} (expected 1 or {INVITE_LINK_VERSION})",
            invite.v
        ));
    }
    if !invite.did.starts_with("did:key:z") {
        return Err("invite did is not a did:key".into());
    }
    if invite.queue.len() != 64 {
        return Err("invite queue is not 256-bit hex".into());
    }
    if invite.expires_at > 0 && invite.expires_at < now_ms() {
        return Err("invite link has expired — ask for a fresh one".into());
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
pub async fn generate_invite(display_label: &str, mode: IdentityMode) -> Result<String, String> {
    let me = ensure_profile().await.map_err(|e| e.to_string())?;
    let my_pub = my_pubkeys()
        .await
        .ok_or_else(|| "no pubkeys (not signed in)".to_string())?;

    // Anonymous mode mints a fresh per-contact identity; Regular presents
    // the real session identity.
    let anon = match mode {
        IdentityMode::Anonymous => Some(mint_anon_identity()?),
        IdentityMode::Regular => None,
    };
    let share_did = match &anon {
        Some(a) => a.did.clone(),
        None => me.did_key.clone(),
    };
    let share_pub = advertised_pubkeys(mode, anon.as_ref(), &my_pub)?;
    let share_name = shared_display_name(mode, &me.name);

    let queue = random_hex(32);
    let recv_pseudonym = random_hex(16);
    let send_pseudonym = random_hex(16);
    let nonce = random_hex(16);

    // Ratchet prekey: a fresh DH keypair we publish in the invite. The accepter
    // uses its public half to bootstrap; we keep the private half stashed (as a
    // prekey-only ratchet state) until the handshake completes the bootstrap.
    let (prekey_priv, prekey_pub) = crypto::ratchet_keypair();

    // Placeholder DID until the handshake reply arrives and gives us
    // the real one. We disambiguate pending invites by queue id.
    let placeholder_did = format!("pending:{queue}");
    let contact = DmContact {
        did: placeholder_did.clone(),
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
        mode,
        anon_identity: anon,
        ratchet_capable: false,
    };
    upsert_contact_record(contact)
        .await
        .map_err(|e| e.to_string())?;

    // Stash the prekey privkey as a not-yet-bootstrapped ratchet state under the
    // placeholder DID. receive_handshake reads it to complete the bootstrap.
    let prekey_state = RatchetState {
        rk: String::new(),
        cks: None,
        ckr: None,
        dhs_priv: hx(&prekey_priv),
        dhs_pub: hx(&prekey_pub),
        dhr_pub: None,
        ns: 0,
        nr: 0,
        pn: 0,
        skipped: Vec::new(),
    };
    let _ = write_ratchet(&placeholder_did, &prekey_state).await;

    // Join our new inbound queue topic so the peer_receiver picks up
    // their handshake reply.
    let _ = peer::join_topic(&format!("{TOPIC_PREFIX_V2}/{queue}")).await;

    let invite = InviteLink {
        v: INVITE_LINK_VERSION,
        queue,
        did: share_did,
        name: share_name,
        keys: share_pub,
        nonce,
        expires_at: now_ms() + INVITE_TTL_MS,
        ratchet: Some(RatchetPrekey {
            dh_pub_b64: B64.encode(prekey_pub),
        }),
    };
    Ok(format!("hey-invite:{}", encode_invite_link(&invite)))
}

/// Accept someone else's invite link. Creates an Active contact, sends
/// the handshake reply (encrypted to their pubkeys) to their queue, and
/// returns the contact's DID so the UI can navigate to the conversation.
///
/// Idempotent on double-click / re-paste: if we already have an Active
/// contact with this DID + pubkeys, we just return its DID without
/// minting a new queue or re-publishing a handshake. Avoids the
/// double-handshake deadlock where Bob's second click would point him
/// at a queue Alice never learns about.
pub async fn accept_invite(token: &str, mode: IdentityMode) -> Result<String, String> {
    let invite = decode_invite_link(token)?;
    let me = ensure_profile().await.map_err(|e| e.to_string())?;
    if invite.did == me.did_key {
        return Err("that's your own invite link".into());
    }
    if let Some(existing) = find_contact(&invite.did).await {
        if existing.status == ContactStatus::Active && existing.peer_pubkeys.is_some() {
            return Ok(existing.did);
        }
    }
    let s = session::current().ok_or_else(|| "not signed in".to_string())?;
    let my_pub = my_pubkeys()
        .await
        .ok_or_else(|| "no pubkeys (not signed in)".to_string())?;

    // Anonymous mode mints a fresh per-contact identity to present to them.
    let anon = match mode {
        IdentityMode::Anonymous => Some(mint_anon_identity()?),
        IdentityMode::Regular => None,
    };
    let (my_did, my_seed_hex) =
        signing_identity(mode, anon.as_ref(), &me.did_key, &s.auth_key_hex)?;
    let share_pub = advertised_pubkeys(mode, anon.as_ref(), &my_pub)?;
    let share_name = shared_display_name(mode, &me.name);

    // Mint OUR queue for receiving from them.
    let my_queue = random_hex(32);
    let my_recv_pseudonym = random_hex(16);
    let my_send_pseudonym = random_hex(16);

    // Ratchet bootstrap (we are the INITIATOR). Only if the invite advertised a
    // prekey — otherwise we negotiate the single-shot path with this peer.
    // SK is derived from a FRESH bootstrap ephemeral (discarded after — must-fix
    // #5) DH'd against the inviter's advertised static X25519, plus an ML-KEM
    // encapsulation to their advertised KEM key. All local even when we are
    // provider-backed (encap needs only their public key).
    let ratchet_bootstrap: Option<(RatchetBootstrap, RatchetState)> = match &invite.ratchet {
        Some(prekey) => {
            let alice_x: [u8; 32] = B64
                .decode(&invite.keys.x25519_pub_b64)
                .map_err(|e| format!("invite x25519 b64: {e}"))?
                .try_into()
                .map_err(|_| "invite x25519 wrong size".to_string())?;
            let alice_kem = B64
                .decode(&invite.keys.ml_kem_pub_b64)
                .map_err(|e| format!("invite ml-kem b64: {e}"))?;
            let prekey_pub: [u8; 32] = B64
                .decode(&prekey.dh_pub_b64)
                .map_err(|e| format!("invite ratchet prekey b64: {e}"))?
                .try_into()
                .map_err(|_| "invite ratchet prekey wrong size".to_string())?;
            let (eph_priv, eph_pub) = crypto::ratchet_keypair();
            let x3dh = crypto::dh(&eph_priv, &alice_x);
            let (kem_ct, kem_ss) = crypto::ml_kem_encapsulate_local(&alice_kem)?;
            let sk = crypto::root_init(&x3dh, &kem_ss);
            let state = ratchet_init_initiator(sk, prekey_pub);
            let bootstrap = RatchetBootstrap {
                eph_pub_b64: B64.encode(eph_pub),
                kem_ct_b64: B64.encode(&kem_ct),
                dh_pub_b64: B64.encode(b32(&state.dhs_pub)?),
            };
            Some((bootstrap, state))
        }
        None => None,
    };

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
        mode,
        anon_identity: anon,
        ratchet_capable: ratchet_bootstrap.is_some(),
    };
    let _ = upsert_contact_record(contact)
        .await
        .map_err(|e| e.to_string())?;

    // Persist the bootstrapped ratchet state under the peer's real DID.
    if let Some((_, state)) = &ratchet_bootstrap {
        write_ratchet(&invite.did, state)
            .await
            .map_err(|e| e.to_string())?;
    }

    // Join our new inbound queue so the receiver picks up their replies.
    let _ = peer::join_topic(&format!("{TOPIC_PREFIX_V2}/{my_queue}")).await;

    // Build & send the handshake reply on THEIR queue. When we bootstrapped a
    // ratchet, the block tells the inviter how to recover SK + our first DH key.
    let mut handshake_body = json!({
        "my_inbound_queue": my_queue,
        "name": share_name,
        "pubkeys": share_pub,
    });
    if let Some((bootstrap, _)) = &ratchet_bootstrap {
        handshake_body["ratchet"] =
            serde_json::to_value(bootstrap).map_err(|e| format!("ratchet block: {e}"))?;
    }

    let inner = build_inner(KIND_HANDSHAKE, &handshake_body, &my_did, &my_seed_hex, None).await?;
    let envelope = encrypt_inner_for_peer(&inner, &invite.keys)?;
    let wire = json!({
        "type": "dm.v2",
        "envelope": envelope,
    })
    .to_string();

    let topic = format!("{TOPIC_PREFIX_V2}/{}", invite.queue);
    let _ = peer::join_topic(&topic).await;
    // Sealed-sender at the provider layer: random pseudonym, not DID.
    // outbox::publish_or_enqueue uses a constant "v2-sealed" placeholder
    // for the outer signature (providers that validate non-empty don't
    // reject; the real sig is inside the envelope). On publish failure
    // the message is stashed in dm/outbox.json and retried by the
    // peer_receiver poll loop.
    let _ = crate::api::outbox::publish_or_enqueue(&topic, &my_send_pseudonym, &wire).await;

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
    /// Ed25519 sig over `canonicalize({kind, body, sender_did, ts[, rh]})`.
    /// `rh` is in the signed set ONLY when present (must-fix #1) — including it
    /// unconditionally would emit `"rh":null` and break the signature on every
    /// pre-ratchet message + every not-yet-upgraded peer.
    sig: String,
    /// Double Ratchet header (sealed + signed). Present ⇒ ratchet message;
    /// absent ⇒ single-shot (legacy) path. Echoed UNSEALED in the wire `rh`
    /// so the receiver can pick the key + bound skips before decrypting; the
    /// two MUST match (checked post-decrypt) or the message is rejected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rh: Option<RatchetHeader>,
}

/// Shared identity namespace for the runtime provider — one did:key per user
/// across every Hey capsule. (Re-exported from runtime so all signing sites
/// use the same value.)
const IDENTITY_NS: &str = crate::runtime::identity_provider::HEY_NAMESPACE;

/// Sign `payload`: with the local Ed25519 seed when `auth_key_hex` is set
/// (local session or a per-contact anonymous identity), or via the runtime
/// identity provider when it is EMPTY (provider-backed session — the key is
/// runtime-held, no passkey tap, the wallet model). One branch point keeps
/// every signing site mode-agnostic.
async fn sign_bytes(payload: &[u8], auth_key_hex: &str) -> Result<String, String> {
    if auth_key_hex.is_empty() {
        let resp = crate::runtime::identity_provider::sign(IDENTITY_NS, payload)
            .await
            .map_err(|e| format!("provider sign: {e}"))?;
        let d = resp.get("data").unwrap_or(&resp);
        d.get("signature_hex")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| "provider sign: no signature_hex".to_string())
    } else {
        let seed = seed32(auth_key_hex)?;
        Ok(sign(payload, &seed))
    }
}

/// Bytes signed for an inner payload. `rh` is added to the canonicalized
/// object ONLY when present (must-fix #1), so a single-shot message signs
/// EXACTLY the bytes it did before the ratchet existed.
fn inner_sign_bytes(kind: &str, body: &Value, sender_did: &str, ts: i64, rh: Option<&RatchetHeader>) -> String {
    let mut obj = json!({
        "kind": kind,
        "body": body,
        "sender_did": sender_did,
        "ts": ts,
    });
    if let Some(h) = rh {
        obj["rh"] = serde_json::to_value(h).unwrap_or(Value::Null);
    }
    canonicalize(&obj)
}

async fn build_inner(
    kind: &str,
    body: &Value,
    sender_did: &str,
    auth_key_hex: &str,
    rh: Option<RatchetHeader>,
) -> Result<InnerPayload, String> {
    let ts = now_ms();
    let to_sign = inner_sign_bytes(kind, body, sender_did, ts, rh.as_ref());
    let sig = sign_bytes(to_sign.as_bytes(), auth_key_hex).await?;
    Ok(InnerPayload {
        kind: kind.into(),
        sender_did: sender_did.into(),
        ts,
        body: body.clone(),
        sig,
        rh,
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
    let to_sign = inner_sign_bytes(
        &inner.kind,
        &inner.body,
        &inner.sender_did,
        inner.ts,
        inner.rh.as_ref(),
    );
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

async fn decrypt_envelope_to_inner(
    env: &HpqEnvelope,
    via: &DecryptVia,
) -> Result<InnerPayload, String> {
    let pt = open_envelope(env, via).await?;
    serde_json::from_str(&pt).map_err(|e| format!("inner deserialize: {e}"))
}

// ── Double Ratchet state machine (M6 stage 2) ────────────────────────
//
// Signal-style Double Ratchet layered onto the v2 sealed-sender wire. It
// changes ONLY what the X25519-half feeding `crypto::derive_key` IS: a
// forward-secret chain message key `mk` instead of a per-message static ECDH.
// The frozen hpq envelope / ChaCha / padding / HKDF_INFO are untouched.
//
//   * The sender's CURRENT ratchet DH pubkey rides UNSEALED in `envelope.eph`.
//   * The cleartext wire `rh = {pn, n}` lets the receiver pick the right key
//     and bound skips BEFORE decrypting (the page number — must-fix #7); a
//     forged `rh` either fails the AEAD (wrong key) or the post-decrypt check
//     against the SEALED+SIGNED `InnerPayload.rh` (which carries dh,pn,n).
//   * Every ongoing ratchet DH key is a LOCAL ephemeral (the provider can't
//     hold a rotating key). The provider/anon key is used only at bootstrap
//     and for the per-message ML-KEM half (`ratchet_kem_ss`).
//
// HONEST SCOPE (must-fix #7): forward secrecy + post-compromise security come
// from the classical X25519 chain ONLY. The per-message ML-KEM seal is to a
// STATIC key — harvest-now-decrypt-later confidentiality + the RK0 PQ floor,
// no FS/PCS. There is NO PCS until the first full DH round-trip the attacker
// didn't observe.

/// The unsealed page-number header. `dh` is base64 and equals `envelope.eph`;
/// it is duplicated here so the SIGNED copy commits the sender to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RatchetHeader {
    /// Base64 — the sender's current ratchet DH public key (== envelope.eph).
    pub dh: String,
    /// Length of the sender's PREVIOUS sending chain (for old-chain skips).
    pub pn: u32,
    /// Index of this message in the sender's current sending chain.
    pub n: u32,
}

/// One out-of-order message key we derived early and stashed. Keyed by the
/// chain pubkey (hex) it belongs to + its index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedKey {
    /// Hex — the peer ratchet pubkey (DHr) of the chain this key belongs to.
    pub dh: String,
    pub n: u32,
    /// Hex — the 32-byte message key.
    pub mk: String,
    /// When stored (ms) — for TTL eviction.
    pub stored_at: i64,
}

/// Per-contact Double Ratchet state. All key material is hex. Persisted in
/// dm/ratchet/<did>.json. A "prekey-only" state (empty `rk`) is written by the
/// inviter at `generate_invite` and completed at `receive_handshake`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatchetState {
    /// Root key (hex 32B). Empty ⇒ a not-yet-bootstrapped prekey stash.
    pub rk: String,
    /// Sending chain key (hex). None until a sending chain exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cks: Option<String>,
    /// Receiving chain key (hex). None until we've received from a peer chain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ckr: Option<String>,
    /// Our current ratchet DH keypair (hex 32B each).
    pub dhs_priv: String,
    pub dhs_pub: String,
    /// Their current ratchet DH public key (hex). None until first received.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dhr_pub: Option<String>,
    #[serde(default)]
    pub ns: u32,
    #[serde(default)]
    pub nr: u32,
    #[serde(default)]
    pub pn: u32,
    #[serde(default)]
    pub skipped: Vec<SkippedKey>,
}

impl RatchetState {
    /// True once SK is established (not just a prekey stash).
    fn is_bootstrapped(&self) -> bool {
        !self.rk.is_empty()
    }
}

/// Ratchet prekey advertised in an invite (the inviter's initial DH public
/// key). Additive + optional, so old invites (no field) simply don't ratchet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatchetPrekey {
    pub dh_pub_b64: String,
}

/// Ratchet bootstrap block carried in a handshake by the accepter: a discarded
/// bootstrap ephemeral + ML-KEM ct (→ SK) and the accepter's first ratchet DH
/// pubkey (so the inviter can establish both chains immediately).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RatchetBootstrap {
    eph_pub_b64: String,
    kem_ct_b64: String,
    dh_pub_b64: String,
}

// ── ratchet hex helpers ──
fn hx(b: &[u8]) -> String {
    bytes_to_hex(b)
}
fn b32(hex: &str) -> Result<[u8; 32], String> {
    seed32(hex)
}

// ── per-contact ratchet store (must-fix #7: own file, never on contacts.json) ──

fn ratchet_path(did: &str) -> String {
    let safe = did.replace(['/', ':'], "_");
    format!("{RATCHET_DIR}/{safe}.json")
}

async fn read_ratchet(did: &str) -> Option<RatchetState> {
    storage::read_json(&ratchet_path(did))
        .await
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value(v).ok())
}

/// Persist ratchet state. The per-app storage path (/api/apps/<id>/storage/)
/// is OVERWRITE-capable — proven by the message-append + mark_read flows that
/// rewrite dm/*.json on every message — so the advance is durable (must-fix
/// #7's "confirm contacts.json is overwrite-capable, not create-only").
async fn write_ratchet(did: &str, st: &RatchetState) -> Result<(), RuntimeError> {
    let v = serde_json::to_value(st).map_err(|e| RuntimeError::new(format!("serialize: {e}")))?;
    storage::write_json(&ratchet_path(did), &v).await
}

async fn remove_ratchet(did: &str) {
    let _ = storage::remove(&ratchet_path(did)).await;
}

// ── pure ratchet core (no I/O — unit-testable by self_test_ratchet) ──

/// Initiator init (the invite ACCEPTER). We hold SK and the inviter's ratchet
/// prekey pub; establish a sending chain at once. Maps to Signal RatchetInitAlice.
fn ratchet_init_initiator(sk: [u8; 32], peer_prekey_pub: [u8; 32]) -> RatchetState {
    let (dhs_priv, dhs_pub) = crypto::ratchet_keypair();
    let (rk, cks) = crypto::kdf_rk(&sk, &crypto::dh(&dhs_priv, &peer_prekey_pub));
    RatchetState {
        rk: hx(&rk),
        cks: Some(hx(&cks)),
        ckr: None,
        dhs_priv: hx(&dhs_priv),
        dhs_pub: hx(&dhs_pub),
        dhr_pub: Some(hx(&peer_prekey_pub)),
        ns: 0,
        nr: 0,
        pn: 0,
        skipped: Vec::new(),
    }
}

/// Responder init (the INVITER). DHs = our published prekey; RK = SK. We then
/// turn the ratchet immediately against the accepter's first ratchet key so we
/// can BOTH send and receive right away (Signal would defer this to first
/// receive — equivalent, since the accepter's first message carries this dh).
/// Maps to Signal RatchetInitBob + one DHRatchet.
fn ratchet_init_responder(
    sk: [u8; 32],
    prekey_priv: [u8; 32],
    prekey_pub: [u8; 32],
    peer_dh_pub: [u8; 32],
) -> Result<RatchetState, String> {
    let mut st = RatchetState {
        rk: hx(&sk),
        cks: None,
        ckr: None,
        dhs_priv: hx(&prekey_priv),
        dhs_pub: hx(&prekey_pub),
        dhr_pub: None,
        ns: 0,
        nr: 0,
        pn: 0,
        skipped: Vec::new(),
    };
    dh_ratchet(&mut st, peer_dh_pub)?;
    Ok(st)
}

/// Turn the DH ratchet on a freshly-seen peer key: finish nothing here (the
/// caller skips the old chain first), derive the new receiving chain, then mint
/// a FRESH sending keypair (the old `dhs_priv` is overwritten and never reused
/// — that discard is what delivers PCS; must-fix #5).
fn dh_ratchet(st: &mut RatchetState, dh_pub: [u8; 32]) -> Result<(), String> {
    let dhs_priv = b32(&st.dhs_priv)?;
    let rk0 = b32(&st.rk)?;
    st.pn = st.ns;
    st.ns = 0;
    st.nr = 0;
    let (rk1, ckr) = crypto::kdf_rk(&rk0, &crypto::dh(&dhs_priv, &dh_pub));
    st.dhr_pub = Some(hx(&dh_pub));
    st.ckr = Some(hx(&ckr));
    let (new_priv, new_pub) = crypto::ratchet_keypair();
    let (rk2, cks) = crypto::kdf_rk(&rk1, &crypto::dh(&new_priv, &dh_pub));
    st.dhs_priv = hx(&new_priv);
    st.dhs_pub = hx(&new_pub);
    st.rk = hx(&rk2);
    st.cks = Some(hx(&cks));
    Ok(())
}

/// Advance the receiving chain up to (but not including) `until`, stashing each
/// skipped message key. Rejects an implausible jump BEFORE any KDF (must-fix
/// #7 — the cleartext `n`/`pn` make this a pre-KDF check, not a bounded loop).
fn skip_message_keys(st: &mut RatchetState, until: u32) -> Result<(), String> {
    if until > st.nr.saturating_add(MAX_SKIP) {
        return Err(format!(
            "ratchet: would skip past MAX_SKIP ({} > {} + {MAX_SKIP})",
            until, st.nr
        ));
    }
    let Some(ckr_hex) = st.ckr.clone() else {
        return Ok(()); // no receiving chain yet — nothing to skip
    };
    let dhr = st
        .dhr_pub
        .clone()
        .ok_or_else(|| "skip without dhr".to_string())?;
    let mut ckr = b32(&ckr_hex)?;
    let now = now_ms();
    while st.nr < until {
        let (mk, ckr_next) = crypto::kdf_ck(&ckr);
        st.skipped.push(SkippedKey {
            dh: dhr.clone(),
            n: st.nr,
            mk: hx(&mk),
            stored_at: now,
        });
        ckr = ckr_next;
        st.nr += 1;
    }
    st.ckr = Some(hx(&ckr));
    evict_skipped(st);
    Ok(())
}

/// TTL + FIFO eviction of stored skipped keys (bounds memory; must-fix #7).
fn evict_skipped(st: &mut RatchetState) {
    let cutoff = now_ms() - SKIPPED_TTL_MS;
    st.skipped.retain(|k| k.stored_at >= cutoff);
    if st.skipped.len() > MAX_SKIPPED_KEYS {
        let drop = st.skipped.len() - MAX_SKIPPED_KEYS;
        st.skipped.drain(0..drop); // oldest first
    }
}

/// Consume a previously-stashed out-of-order key for (`dh_hex`, `n`), if any.
fn try_skipped(st: &mut RatchetState, dh_hex: &str, n: u32) -> Result<Option<[u8; 32]>, String> {
    if let Some(pos) = st.skipped.iter().position(|k| k.dh == dh_hex && k.n == n) {
        let k = st.skipped.remove(pos);
        Ok(Some(b32(&k.mk)?))
    } else {
        Ok(None)
    }
}

/// Advance the SENDING chain one step → (message key, header to put on the
/// wire). Caller MUST persist the advanced state BEFORE using `mk` for anything
/// durable, so a crash can never reuse `ns`/`mk` (must-fix #5: no mk reuse).
fn ratchet_step_send(st: &mut RatchetState) -> Result<([u8; 32], RatchetHeader), String> {
    let cks = st
        .cks
        .clone()
        .ok_or_else(|| "ratchet has no sending chain yet".to_string())?;
    let (mk, cks_next) = crypto::kdf_ck(&b32(&cks)?);
    st.cks = Some(hx(&cks_next));
    let header = RatchetHeader {
        dh: B64.encode(b32(&st.dhs_pub)?),
        pn: st.pn,
        n: st.ns,
    };
    st.ns += 1;
    Ok((mk, header))
}

/// Advance the RECEIVING ratchet to position (`dh`, `n`) and return its message
/// key. Operates on a CLONE supplied by the caller: on any failure (bad jump,
/// AEAD mismatch downstream, old/garbage epoch) the caller discards the clone,
/// so a forged/replayed message can never corrupt the committed state.
fn ratchet_step_recv(
    st: &mut RatchetState,
    dh_hex: &str,
    dh_bytes: [u8; 32],
    pn: u32,
    n: u32,
) -> Result<[u8; 32], String> {
    // 1. Out-of-order: a key we already derived and stashed.
    if let Some(mk) = try_skipped(st, dh_hex, n)? {
        return Ok(mk);
    }
    // 2. GLOBAL pre-KDF work bound (must-fix #7). The most keys THIS one message
    //    can force us to derive is (skip the old chain to pn) + (skip the new
    //    chain to n). Because dh_ratchet resets nr to 0, a per-call cap would
    //    allow up to 2*MAX_SKIP; bound the COMBINED total here — before any
    //    kdf_ck runs — so a forged cleartext counter (eph/pn/n are unauthenticated
    //    until the AEAD, which runs later on a clone) can't drive unbounded CPU.
    let new_epoch = st.dhr_pub.as_deref() != Some(dh_hex);
    let old_skip = if new_epoch { pn.saturating_sub(st.nr) } else { 0 };
    let new_start = if new_epoch { 0 } else { st.nr };
    let total_skip = old_skip.saturating_add(n.saturating_sub(new_start));
    if total_skip > MAX_SKIP {
        return Err(format!(
            "ratchet: combined skip {total_skip} exceeds MAX_SKIP {MAX_SKIP}"
        ));
    }
    // 3. A new DH epoch ⇒ finish the previous receiving chain up to pn, turn.
    if new_epoch {
        skip_message_keys(st, pn)?;
        dh_ratchet(st, dh_bytes)?;
    }
    // 4. Skip within the current chain to n, then derive the key AT n.
    skip_message_keys(st, n)?;
    let ckr = st
        .ckr
        .clone()
        .ok_or_else(|| "ratchet has no receiving chain".to_string())?;
    let (mk, ckr_next) = crypto::kdf_ck(&b32(&ckr)?);
    st.ckr = Some(hx(&ckr_next));
    st.nr += 1;
    Ok(mk)
}

// ── Attachments (M7): E2E files via the content store ────────────────
//
// Send: encrypt each file under a fresh key (crypto::encrypt_attachment),
// upload the CIPHERTEXT to the content store, and carry the {cid,key,meta}
// Attachment INSIDE the sealed message body. Receive: the Attachment rides in
// the decrypted body; the UI calls fetch_attachment to pull + decrypt on render.
// The store/relay only ever holds opaque ciphertext.

/// Max plaintext size we upload in one shot (no chunking yet). 25 MiB.
const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;

/// Encrypt + upload one file; returns the sealed reference to embed in a message.
pub async fn upload_attachment(name: &str, mime: &str, bytes: &[u8]) -> Result<Attachment, String> {
    if bytes.is_empty() {
        return Err("empty attachment".into());
    }
    if bytes.len() > MAX_ATTACHMENT_BYTES {
        return Err(format!(
            "attachment too large ({} bytes; max {MAX_ATTACHMENT_BYTES})",
            bytes.len()
        ));
    }
    let (ciphertext, key_b64) = crypto::encrypt_attachment(bytes)?;
    // Upload ciphertext under an opaque filename (the real name is sealed in the
    // message, never handed to the store); pin so the peer can fetch it.
    let resp = crate::runtime::content::add_bytes(&ciphertext, "att.bin", true)
        .await
        .map_err(|e| format!("attachment upload: {e}"))?;
    let cid = crate::runtime::content::extract_cid(&resp)
        .ok_or_else(|| "attachment upload: no cid in response".to_string())?;
    Ok(Attachment {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.chars().take(255).collect(),
        mime: mime.chars().take(128).collect(),
        size: bytes.len() as u64,
        cid,
        key_b64,
    })
}

/// Fetch + decrypt one attachment's plaintext bytes (render path, both sides).
pub async fn fetch_attachment(att: &Attachment) -> Result<Vec<u8>, String> {
    let ciphertext = crate::runtime::content::get_bytes(&att.cid, None)
        .await
        .map_err(|e| format!("attachment fetch: {e}"))?;
    crypto::decrypt_attachment(&ciphertext, &att.key_b64)
}

/// Parse the `attachments` array out of a decrypted inner-payload body.
fn attachments_from_body(body: &Value) -> Vec<Attachment> {
    body.get("attachments")
        .and_then(|v| serde_json::from_value::<Vec<Attachment>>(v.clone()).ok())
        .unwrap_or_default()
}

// ── Public send / receive entry points ───────────────────────────────

/// Build the wire string for a ratchet message: advance the sending chain,
/// PERSIST the advanced state before `mk` is used (so a crash can never reuse
/// it — must-fix #5), then seal the signed inner payload under `mk` + a fresh
/// ML-KEM encapsulation to the peer's STATIC kem key. The cleartext `rh`
/// carries the page number (`pn`,`n`); the sealed `InnerPayload.rh` carries the
/// same triple under the signature.
async fn build_ratchet_wire(
    peer_did: &str,
    peer_keys: &PeerKeys,
    body: &Value,
    my_did: &str,
    my_seed_hex: &str,
) -> Result<String, String> {
    let mut st = read_ratchet(peer_did)
        .await
        .filter(|s| s.is_bootstrapped())
        .ok_or_else(|| {
            "ratchet-capable contact has no ratchet state (refusing to downgrade)".to_string()
        })?;
    let (mk, header) = ratchet_step_send(&mut st)?;
    write_ratchet(peer_did, &st)
        .await
        .map_err(|e| e.to_string())?;
    let inner = build_inner(KIND_MESSAGE, body, my_did, my_seed_hex, Some(header.clone())).await?;
    let plaintext = serde_json::to_string(&inner).map_err(|e| format!("inner json: {e}"))?;
    let recipient_kem = B64
        .decode(&peer_keys.ml_kem_pub_b64)
        .map_err(|e| format!("peer ml-kem b64: {e}"))?;
    let dhs_pub: [u8; 32] = B64
        .decode(&header.dh)
        .map_err(|e| format!("ratchet dh b64: {e}"))?
        .try_into()
        .map_err(|_| "ratchet dh wrong size".to_string())?;
    let envelope = crypto::encrypt_with_mk(&plaintext, &mk, &recipient_kem, &dhs_pub)?;
    Ok(json!({
        "type": "dm.v2",
        "rh": { "pn": header.pn, "n": header.n },
        "envelope": envelope,
    })
    .to_string())
}

/// Send a message. v2 path (sealed sender, per-pair queue) is used when
/// the contact is is_v2_active(); otherwise we fall through to the
/// legacy v1 path for back-compat with contacts created before queues.
pub async fn send_message(peer_did: &str, text: &str) -> Result<DmMessage, String> {
    send_message_inner(peer_did, text, Vec::new()).await
}

/// Send a message carrying E2E attachments. Upload each file with
/// `upload_attachment` first, then pass the refs here. Attachments require a
/// metadata-safe (v2) contact.
pub async fn send_message_with_attachments(
    peer_did: &str,
    text: &str,
    attachments: Vec<Attachment>,
) -> Result<DmMessage, String> {
    send_message_inner(peer_did, text, attachments).await
}

async fn send_message_inner(
    peer_did: &str,
    text: &str,
    attachments: Vec<Attachment>,
) -> Result<DmMessage, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() && attachments.is_empty() {
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
    if !attachments.is_empty() && !use_v2 {
        return Err("attachments need a metadata-safe (v2) contact".into());
    }

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
        attachments: attachments.clone(),
    };
    let preview = if plain_text.is_empty() && !attachments.is_empty() {
        format!("📎 {}", attachments[0].name)
    } else {
        plain_text.clone()
    };
    let mut conv = read_conversation(peer_did).await;
    conv.push(msg.clone());
    write_conversation(peer_did, &conv)
        .await
        .map_err(|e| e.to_string())?;
    touch_contact_message(peer_did, &preview, msg.ts, 0)
        .await
        .map_err(|e| e.to_string())?;

    if use_v2 {
        let c = contact.unwrap();
        let queue = c.their_inbound_queue.clone().unwrap();
        let send_pseudonym = c
            .my_send_pseudonym
            .clone()
            .unwrap_or_else(|| "anonymous".into());
        let peer_keys = c.peer_pubkeys.clone().unwrap();

        // Sign as the identity this contact knows us by (real DID in
        // Regular mode, the per-contact ephemeral DID in Anonymous mode).
        let (my_did, my_seed_hex) =
            signing_identity(c.mode, c.anon_identity.as_ref(), &me.did_key, &s.auth_key_hex)?;
        let body = if attachments.is_empty() {
            json!({ "text": plain_text })
        } else {
            json!({ "text": plain_text, "attachments": attachments })
        };

        // Ratchet-capable contacts ALWAYS ratchet (no silent downgrade —
        // must-fix #6); others use the single-shot seal to static keys.
        let wire = if c.ratchet_capable {
            build_ratchet_wire(peer_did, &peer_keys, &body, &my_did, &my_seed_hex).await?
        } else {
            let inner = build_inner(KIND_MESSAGE, &body, &my_did, &my_seed_hex, None).await?;
            let envelope = encrypt_inner_for_peer(&inner, &peer_keys)?;
            json!({ "type": "dm.v2", "envelope": envelope }).to_string()
        };

        let topic = format!("{TOPIC_PREFIX_V2}/{queue}");
        let _ = peer::join_topic(&topic).await;
        let _ = crate::api::outbox::publish_or_enqueue(&topic, &send_pseudonym, &wire).await;
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

    let evt = crate::events::create_signed_event("dm.message", payload, &s.auth_key_hex).await?;
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
        attachments: Vec::new(), // legacy v1 wire carries no attachments
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
    let queue_id = queue_id_from_topic(topic);

    // A cleartext `rh` (the page number) marks a Double Ratchet message — always
    // a KIND_MESSAGE. Control messages (handshake/welcome) never carry rh and
    // go down the single-shot path below.
    if let Some(rh) = v.get("rh") {
        let pn = u32_field(rh, "pn")?;
        let n = u32_field(rh, "n")?;
        return receive_ratchet_message(queue_id, &envelope, pn, n).await;
    }

    // No rh ⇒ single-shot. Anonymous contacts seal to a per-contact ephemeral
    // pubkey, so the decrypt keys are chosen by the queue this landed on.
    let via = decrypt_via_for_queue(queue_id).await?;
    let inner = decrypt_envelope_to_inner(&envelope, &via).await?;
    if !verify_inner(&inner) {
        return Err("inner signature mismatch".into());
    }
    match inner.kind.as_str() {
        KIND_MESSAGE => {
            let queue_id = queue_id.ok_or_else(|| "bad topic".to_string())?;
            // Defense in depth: the sender_did must own the queue this landed
            // on. Stops a stranger delivering via a leaked queue id.
            let owner = list_contacts().await.into_iter().find(|c| {
                c.did == inner.sender_did
                    && c.my_inbound_queue.as_deref() == Some(queue_id)
                    && c.status == ContactStatus::Active
            });
            let owner = owner.ok_or_else(|| "sender does not match queue owner".to_string())?;
            // Downgrade protection (must-fix #6): a ratchet-capable contact must
            // never be served a single-shot message — refuse rather than fall
            // back to the no-PCS path (the OOB invite is only TOFU-authenticated).
            if owner.ratchet_capable {
                return Err(
                    "refusing single-shot message from a ratchet-capable contact (downgrade)"
                        .into(),
                );
            }
            let text = inner.body.get("text").and_then(|t| t.as_str()).unwrap_or("");
            let atts = attachments_from_body(&inner.body);
            if text.is_empty() && atts.is_empty() {
                return Err("message body has neither text nor attachments".into());
            }
            store_incoming_message(&inner.sender_did, text, inner.ts, None, atts).await
        }
        KIND_HANDSHAKE => {
            let queue_id = queue_id.ok_or_else(|| "bad topic".to_string())?;
            receive_handshake(&inner, queue_id).await
        }
        KIND_WELCOME => receive_welcome(&inner).await,
        other => Err(format!("unknown inner kind: {other}")),
    }
}

/// Parse a non-negative u32 wire field, rejecting anything out of range BEFORE
/// it can reach the ratchet (a forged 2^40 counter must not wrap to a small u32
/// and slip under the MAX_SKIP cap).
fn u32_field(obj: &Value, key: &str) -> Result<u32, String> {
    obj.get(key)
        .and_then(|x| x.as_u64())
        .filter(|&x| x <= u32::MAX as u64)
        .map(|x| x as u32)
        .ok_or_else(|| format!("rh missing or out-of-range {key}"))
}

/// Deterministic per-message id derived from the sealed ciphertext. Identical
/// across redeliveries of the SAME envelope (fresh nonce/KEM make distinct
/// messages differ), so it dedups the non-idempotent ratchet advance.
fn ratchet_dedup_id(env: &HpqEnvelope) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(env.ct.as_bytes());
    h.update(env.n.as_bytes());
    format!("rdm:{}", bytes_to_hex(&h.finalize()[..16]))
}

/// True if the conversation with `sender` already holds a message with `id`.
async fn conv_has(sender: &str, id: &str) -> bool {
    read_conversation(sender).await.iter().any(|m| m.id == id)
}

/// Append a received message to its conversation + bump the contact preview.
/// When `dedup_id` is set, a message already bearing that id is treated as a
/// redelivery and NOT re-appended (the caller still persists ratchet state).
async fn store_incoming_message(
    sender_did: &str,
    text: &str,
    ts: i64,
    dedup_id: Option<&str>,
    attachments: Vec<Attachment>,
) -> Result<(), String> {
    let mut conv = read_conversation(sender_did).await;
    if let Some(id) = dedup_id {
        if conv.iter().any(|m| m.id == id) {
            return Ok(()); // redelivery — already stored
        }
    }
    let msg = DmMessage {
        id: dedup_id
            .map(|s| s.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        text: text.chars().take(4096).collect(),
        ts,
        mine: false,
        encrypted: true,
        attachments,
    };
    let preview = if msg.text.is_empty() && !msg.attachments.is_empty() {
        format!("📎 {}", msg.attachments[0].name)
    } else {
        msg.text.clone()
    };
    conv.push(msg.clone());
    write_conversation(sender_did, &conv)
        .await
        .map_err(|e| e.to_string())?;
    touch_contact_message(sender_did, &preview, msg.ts, 1)
        .await
        .map_err(|e| e.to_string())
}

/// Receive a Double Ratchet message. The whole ratchet advance runs on a CLONE
/// and is committed only after an AUTHENTICATED decrypt + signature + header
/// check — so a forged/replayed/old-epoch envelope can never corrupt the
/// committed ratchet (it just fails and the clone is dropped). On success the
/// plaintext is stored FIRST and the advanced ratchet persisted LAST, so a
/// crash in between is healed by redelivery (re-derive + dedup) rather than
/// losing the message (must-fix #4).
async fn receive_ratchet_message(
    queue_id: Option<&str>,
    envelope: &HpqEnvelope,
    pn: u32,
    n: u32,
) -> Result<(), String> {
    let queue_id = queue_id.ok_or_else(|| "bad topic".to_string())?;
    // The contact owning this inbound queue — its did is the peer's signing did
    // AND the key the ratchet state is filed under.
    let c = list_contacts()
        .await
        .into_iter()
        .find(|c| {
            c.my_inbound_queue.as_deref() == Some(queue_id) && c.status == ContactStatus::Active
        })
        .ok_or_else(|| "ratchet message on an unowned queue".to_string())?;
    if !c.ratchet_capable {
        return Err("ratchet message for a non-ratchet contact".into());
    }
    let st0 = read_ratchet(&c.did)
        .await
        .filter(|s| s.is_bootstrapped())
        .ok_or_else(|| "ratchet message but no ratchet state".to_string())?;

    // dh comes from the UNSEALED envelope.eph (and is re-checked against the
    // sealed+signed header after decrypt).
    let dh_bytes: [u8; 32] = B64
        .decode(&envelope.eph)
        .map_err(|e| format!("eph b64: {e}"))?
        .try_into()
        .map_err(|_| "eph wrong size".to_string())?;
    let dh_hex = hx(&dh_bytes);
    let dedup_id = ratchet_dedup_id(envelope);

    // No pre-decrypt short-circuit, and we NEVER store anything that didn't open
    // + verify: a message we can't decrypt is indistinguishable from a forgery
    // (the relay/anyone who learns the queue id can craft one with the peer's
    // public eph + any n), so storing an "undecryptable" marker for it would let
    // them inject unauthenticated lines into the conversation. Instead the whole
    // advance runs on a CLONE that is committed only after an AUTHENTICATED
    // decrypt+verify; if that fails we either no-op a genuine redelivery (the
    // conversation already holds this exact ciphertext) or return an explicit
    // Err (logged by the receive loop) — never a silent Ok, never a silent drop.
    // A genuinely lost message (its skipped key was evicted by TTL/FIFO before it
    // arrived) lands here too: surfaced as a logged Err, not invented UI.
    let mut st = st0.clone();
    let mk = match ratchet_step_recv(&mut st, &dh_hex, dh_bytes, pn, n) {
        Ok(mk) => mk,
        Err(e) => {
            if conv_has(&c.did, &dedup_id).await {
                return Ok(()); // redelivery of an already-stored message — benign
            }
            return Err(format!("ratchet advance (undecryptable/forged, dropped): {e}"));
        }
    };
    let via = decrypt_via_for_contact(&c)?;
    let kem_ss = ratchet_kem_ss(envelope, &via).await?;
    let plaintext = match crypto::open_with_secrets(envelope, &mk, &kem_ss) {
        Ok(pt) => pt,
        Err(e) => {
            if conv_has(&c.did, &dedup_id).await {
                return Ok(());
            }
            return Err(format!("ratchet decrypt (undecryptable/forged, dropped): {e}"));
        }
    };

    let inner: InnerPayload =
        serde_json::from_str(&plaintext).map_err(|e| format!("inner deserialize: {e}"))?;
    if !verify_inner(&inner) {
        return Err("inner signature mismatch".into());
    }
    // The sealed+signed header must match the eph we keyed on AND the cleartext
    // page number we advanced to — closes any wire/seal tampering.
    let want = RatchetHeader {
        dh: envelope.eph.clone(),
        pn,
        n,
    };
    if inner.rh.as_ref() != Some(&want) {
        return Err("ratchet header mismatch (sealed vs wire)".into());
    }
    if inner.kind != KIND_MESSAGE {
        return Err("ratchet wire carried a non-message kind".into());
    }
    if inner.sender_did != c.did {
        return Err("ratchet sender does not match queue owner".into());
    }
    let text = inner.body.get("text").and_then(|t| t.as_str()).unwrap_or("");
    let atts = attachments_from_body(&inner.body);
    if text.is_empty() && atts.is_empty() {
        return Err("message body has neither text nor attachments".into());
    }

    // Store plaintext FIRST, persist the consumed advance LAST.
    store_incoming_message(&c.did, text, inner.ts, Some(&dedup_id), atts).await?;
    write_ratchet(&c.did, &st)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Handle a handshake reply that landed on one of OUR queues. The
/// queue id (NOT the sender_did) is the disambiguator — when we
/// minted the invite we didn't know who the recipient would be.
///
/// After promoting the contact to Active, we ROTATE: mint a fresh
/// Alice-side queue, send a `welcome` message on Bob's queue telling
/// him to switch to it, and retire the original invite queue
/// (peer_receiver::forget_topic + outbox::purge_topic). The original
/// invite queue is single-use from this moment on — even if the
/// invite link leaks to a third party, sending on it goes nowhere.
/// Complete the responder side of the ratchet bootstrap from a handshake's
/// `ratchet` block + the prekey we stashed at generate_invite. Returns Ok(true)
/// when a ratchet state was written, Ok(false) when there's nothing to
/// bootstrap (no stashed prekey), Err on a hard failure.
async fn bootstrap_responder_ratchet(
    c: &DmContact,
    placeholder_did: &str,
    rb: &Value,
    real_did: &str,
) -> Result<bool, String> {
    let Some(prekey_state) = read_ratchet(placeholder_did).await else {
        return Ok(false); // no stashed prekey — negotiate single-shot
    };
    if prekey_state.is_bootstrapped() {
        return Ok(false); // already a full state; nothing to do
    }
    let field = |k: &str| -> Result<String, String> {
        rb.get(k)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| format!("ratchet block missing {k}"))
    };
    let eph_pub = B64
        .decode(field("eph_pub_b64")?)
        .map_err(|e| format!("ratchet eph b64: {e}"))?;
    let kem_ct = B64
        .decode(field("kem_ct_b64")?)
        .map_err(|e| format!("ratchet kem_ct b64: {e}"))?;
    let bob_dh: [u8; 32] = B64
        .decode(field("dh_pub_b64")?)
        .map_err(|e| format!("ratchet dh b64: {e}"))?
        .try_into()
        .map_err(|_| "ratchet dh wrong size".to_string())?;

    let via = decrypt_via_for_contact(c)?;
    let (x3dh, kem_ss) = shared_secrets(&via, &eph_pub, &kem_ct).await?;
    let sk = crypto::root_init(&x3dh, &kem_ss);
    let prekey_priv = b32(&prekey_state.dhs_priv)?;
    let prekey_pub = b32(&prekey_state.dhs_pub)?;
    let state = ratchet_init_responder(sk, prekey_priv, prekey_pub, bob_dh)?;
    // Write the bootstrapped state under the peer's real DID, but DO NOT remove
    // the prekey stash here — the caller removes it only AFTER write_contacts
    // durably promotes the contact (so a contacts-write failure leaves the stash
    // intact and a redelivered handshake can re-bootstrap, rather than wedging
    // the contact in PendingInvite with its prekey already gone).
    write_ratchet(real_did, &state)
        .await
        .map_err(|e| e.to_string())?;
    Ok(true)
}

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
        // Either a replayed handshake (sender retried on top of an
        // already-promoted contact) or a stranger guessed the queue
        // id (astronomically unlikely with 256 bits of entropy). Log
        // so the debug console shows what happened.
        web_sys::console::warn_1(&wasm_bindgen::JsValue::from_str(&format!(
            "[hey-social] handshake replay or stranger on queue {} from {}",
            on_queue, inner.sender_did
        )));
        return Ok(());
    };

    // Rotate: mint Alice's ongoing queue and pseudonyms; the old
    // invite queue retires below.
    let new_queue = random_hex(32);
    let new_recv_pseudonym = random_hex(16);

    let mut c = list.remove(pos);
    let old_queue = c.my_inbound_queue.clone();
    let placeholder_did = c.did.clone(); // "pending:<queue>" — keys the prekey stash
    c.did = inner.sender_did.clone();
    c.their_inbound_queue = Some(their_queue.to_string());
    c.peer_pubkeys = Some(their_keys.clone());
    c.status = ContactStatus::Active;
    if c.name.is_empty() || c.name.starts_with("pending:") {
        c.name = their_name.into();
    }
    c.last_ts = inner.ts;
    c.last_preview = "Invite accepted ✓".into();
    c.my_inbound_queue = Some(new_queue.clone());
    c.my_recv_pseudonym = Some(new_recv_pseudonym);

    // Ratchet bootstrap (we are the RESPONDER), ATOMIC with promotion. The
    // accepter unilaterally committed ratchet_capable=true (its bootstrap is
    // purely local, can't fail), so if the handshake offers a ratchet we MUST
    // either bootstrap successfully OR refuse to promote — never go Active with
    // ratchet_capable=false while the peer is capable, which would brick the
    // conversation both ways with no recovery. On failure we return Err WITHOUT
    // writing contacts (the in-memory promotion is discarded) and WITHOUT
    // removing the prekey stash, so the contact stays PendingInvite and a
    // redelivered handshake retries once the provider recovers. Recovery uses
    // OUR key material via decrypt_via_for_contact — anon ⇒ local anon key,
    // provider-backed ⇒ runtime, else local seed (must-fix #3: anon never
    // touches the provider). A provider-down blip almost always fails the
    // handshake DECRYPT first (same provider ops), so this path is rare.
    let offered_ratchet = inner.body.get("ratchet").cloned();
    let ratchet_capable = if let Some(rb) = offered_ratchet {
        match bootstrap_responder_ratchet(&c, &placeholder_did, &rb, &inner.sender_did).await {
            Ok(true) => true,
            Ok(false) => {
                return Err(
                    "responder ratchet bootstrap: prekey stash missing — re-invite to re-establish"
                        .into(),
                );
            }
            Err(e) => {
                return Err(format!(
                    "responder ratchet bootstrap failed (handshake will retry once recovered): {e}"
                ));
            }
        }
    } else {
        // Peer didn't advertise ratchet — single-shot; drop the prekey stash.
        remove_ratchet(&placeholder_did).await;
        false
    };
    c.ratchet_capable = ratchet_capable;

    // Capture our identity for this contact before moving `c` into the list:
    // the welcome we send below must be signed as the SAME identity the peer
    // knows us by (real DID in Regular, ephemeral DID in Anonymous).
    let my_mode = c.mode;
    let my_anon = c.anon_identity.clone();
    list.push(c);
    list.sort_by(|a, b| b.last_ts.cmp(&a.last_ts));
    write_contacts(&list).await.map_err(|e| e.to_string())?;

    // Contact is now durably promoted — only NOW retire the prekey stash (the
    // bootstrap wrote the real-DID ratchet state but deliberately left the stash
    // so a write_contacts failure above would have left a re-bootstrappable
    // PendingInvite). On the no-ratchet path the stash was already dropped.
    if ratchet_capable {
        remove_ratchet(&placeholder_did).await;
    }

    // Send the welcome on BOB's queue so he learns Alice's new queue.
    let s = match session::current() {
        Some(s) => s,
        None => return Ok(()),
    };
    let me_real = inner_to_my_did().unwrap_or_default();
    let (welcome_did, welcome_seed) =
        match signing_identity(my_mode, my_anon.as_ref(), &me_real, &s.auth_key_hex) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };
    let welcome_body = json!({ "my_inbound_queue": new_queue });
    if !welcome_did.is_empty() {
        if let Ok(welcome_inner) =
            build_inner(KIND_WELCOME, &welcome_body, &welcome_did, &welcome_seed, None).await
        {
            if let Ok(envelope) = encrypt_inner_for_peer(&welcome_inner, &their_keys) {
                let wire = json!({
                    "type": "dm.v2",
                    "envelope": envelope,
                })
                .to_string();
                let bob_topic = format!("{TOPIC_PREFIX_V2}/{their_queue}");
                let send_pseudonym = random_hex(16);
                let _ = peer::join_topic(&bob_topic).await;
                let _ = crate::api::outbox::publish_or_enqueue(
                    &bob_topic,
                    &send_pseudonym,
                    &wire,
                )
                .await;
            }
        }
    }

    // Retire the original invite queue. forget_topic clears the
    // join-once cache + tells the provider we're not listening
    // anymore; purge_topic drops anything still pending in the outbox
    // for that topic.
    if let Some(old) = old_queue {
        let old_topic = format!("{TOPIC_PREFIX_V2}/{old}");
        crate::peer_receiver::forget_topic(&old_topic).await;
        crate::api::outbox::purge_topic(&old_topic).await;
    }

    Ok(())
}

/// Process a `welcome` payload: Bob learns Alice's rotated queue and
/// updates `their_inbound_queue` so his next send lands on the right
/// destination. Outbox items still pointing at Alice's old queue are
/// dropped — Alice isn't listening there anymore.
async fn receive_welcome(inner: &InnerPayload) -> Result<(), String> {
    let new_queue = inner
        .body
        .get("my_inbound_queue")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "welcome missing my_inbound_queue".to_string())?;
    let mut list = list_contacts().await;
    let Some(c) = list.iter_mut().find(|c| c.did == inner.sender_did) else {
        web_sys::console::warn_1(&wasm_bindgen::JsValue::from_str(&format!(
            "[hey-social] welcome from unknown {}",
            inner.sender_did
        )));
        return Ok(());
    };
    let prev = c.their_inbound_queue.clone();
    c.their_inbound_queue = Some(new_queue.to_string());
    write_contacts(&list).await.map_err(|e| e.to_string())?;
    if let Some(prev) = prev {
        if prev != new_queue {
            let stale_topic = format!("{TOPIC_PREFIX_V2}/{prev}");
            crate::api::outbox::purge_topic(&stale_topic).await;
        }
    }
    Ok(())
}

/// Recover the signed-in user's DID from the session. Returns None if
/// signed out or the auth-key is malformed.
fn inner_to_my_did() -> Option<String> {
    let s = session::current()?;
    let seed_vec = hex_to_bytes(&s.auth_key_hex).ok()?;
    if seed_vec.len() != 32 {
        return None;
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&seed_vec);
    let kp = ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new(seed));
    let pk_bytes: [u8; 32] = *kp.pk;
    Some(crate::identity::public_key_to_did_key(&pk_bytes))
}

// ── Self-test: v2 wire-format crypto roundtrip ───────────────────────
//
// Builds an inner payload signed with the current session's key,
// encrypts it to our own pubkeys, serializes the wire envelope,
// parses it back, decrypts, verifies the inner sig, and confirms the
// recovered payload matches. Also exercises the invite-link codec
// round-trip. Returns Ok("✓ …") or Err describing the failure step.
//
// This catches: bad JSON encoding of InnerPayload, broken hybrid PQ
// keys in the current session, sig-verify regressions, and invite-
// link base64url/JSON drift. It does NOT exercise the runtime peer
// provider — for that you need two real instances.

pub async fn self_test_v2() -> Result<String, String> {
    let me = ensure_profile().await.map_err(|e| format!("profile: {e}"))?;
    let s = session::current().ok_or_else(|| "not signed in".to_string())?;
    let my_pub = my_public_pubkeys().ok_or_else(|| "no pubkeys".to_string())?;

    let body = json!({ "text": "self-test ping" });
    let inner = build_inner(KIND_MESSAGE, &body, &me.did_key, &s.auth_key_hex, None)
        .await
        .map_err(|e| format!("build_inner: {e}"))?;

    let envelope = encrypt_inner_for_peer(&inner, &my_pub)
        .map_err(|e| format!("encrypt: {e}"))?;
    let wire = json!({
        "type": "dm.v2",
        "envelope": envelope,
    })
    .to_string();

    let v: Value = serde_json::from_str(&wire).map_err(|e| format!("wire reparse: {e}"))?;
    if v.get("type").and_then(|t| t.as_str()) != Some("dm.v2") {
        return Err("type field missing on reparse".into());
    }
    let env_val = v.get("envelope").ok_or_else(|| "no envelope on reparse".to_string())?;
    let env_back: HpqEnvelope = serde_json::from_value(env_val.clone())
        .map_err(|e| format!("envelope reparse: {e}"))?;
    let session_keys = load_my_keys().map_err(|e| format!("load keys: {e}"))?;
    let inner_back = decrypt_envelope_to_inner(&env_back, &DecryptVia::Local(session_keys))
        .await
        .map_err(|e| format!("decrypt: {e}"))?;
    if !verify_inner(&inner_back) {
        return Err("inner signature did NOT verify".into());
    }
    if inner_back.sender_did != me.did_key {
        return Err(format!(
            "sender_did mismatch: got {} expected {}",
            inner_back.sender_did, me.did_key
        ));
    }
    let recovered = inner_back
        .body
        .get("text")
        .and_then(|t| t.as_str())
        .unwrap_or("");
    if recovered != "self-test ping" {
        return Err(format!("text mismatch: got {recovered:?}"));
    }

    // ── Anonymous-mode round-trip: ephemeral identity, no real-DID leak ──
    let anon = mint_anon_identity().map_err(|e| format!("anon mint: {e}"))?;
    if anon.did == me.did_key {
        return Err("anon identity collided with the real DID".into());
    }
    let anon_pub = anon_pubkeys(&anon).map_err(|e| format!("anon pub: {e}"))?;
    let (asign_did, asign_seed) =
        signing_identity(IdentityMode::Anonymous, Some(&anon), &me.did_key, &s.auth_key_hex)
            .map_err(|e| format!("anon signing id: {e}"))?;
    if asign_did != anon.did {
        return Err("anon signing did != ephemeral did".into());
    }
    let ainner = build_inner(
        KIND_MESSAGE,
        &json!({ "text": "anon ping" }),
        &asign_did,
        &asign_seed,
        None,
    )
    .await
    .map_err(|e| format!("anon build_inner: {e}"))?;
    if ainner.sender_did != anon.did {
        return Err("anon inner.sender_did is not the ephemeral did".into());
    }
    let aenv =
        encrypt_inner_for_peer(&ainner, &anon_pub).map_err(|e| format!("anon encrypt: {e}"))?;
    let akeys = anon_user_keys(&anon).map_err(|e| format!("anon keys: {e}"))?;
    let aback = decrypt_envelope_to_inner(&aenv, &DecryptVia::Local(akeys))
        .await
        .map_err(|e| format!("anon decrypt: {e}"))?;
    if !verify_inner(&aback) {
        return Err("anon inner signature did NOT verify".into());
    }
    if aback.sender_did == me.did_key || aback.sender_did != anon.did {
        return Err("anon round-trip leaked or mismatched the sender did".into());
    }

    // Invite-link codec roundtrip — independent of envelope crypto.
    let invite = InviteLink {
        v: INVITE_LINK_VERSION,
        queue: random_hex(32),
        did: me.did_key.clone(),
        name: "self-test".into(),
        keys: my_pub,
        nonce: random_hex(16),
        expires_at: now_ms() + INVITE_TTL_MS,
        ratchet: None,
    };
    let encoded = format!("hey-invite:{}", encode_invite_link(&invite));
    let decoded = decode_invite_link(&encoded).map_err(|e| format!("invite decode: {e}"))?;
    if decoded.did != invite.did || decoded.queue != invite.queue || decoded.nonce != invite.nonce {
        return Err("invite link round-trip mismatch".into());
    }
    if decoded.expires_at != invite.expires_at {
        return Err("invite expires_at mismatch".into());
    }
    if !crate::api::outbox::schema_roundtrip_ok() {
        return Err("outbox schema roundtrip broken".into());
    }

    Ok("✓ v2 envelope + anon round-trip + invite codec + outbox schema OK".into())
}

// ── Double Ratchet self-test (pure, no storage/provider) ─────────────
//
// Drives two in-memory ratchet states through bootstrap + a multi-message
// exchange and asserts the must-fix failure modes. Touches no session, storage,
// or identity provider — but it is NOT host-pure: the state machine stamps
// skipped-key timestamps via js_sys::Date::now, so run it from a wasm debug
// console like self_test_v2 (not native `cargo test`). A wasm_bindgen_test
// wrapper to gate it in CI is a TODO.

/// Seal `text` as a ratchet message from `st` to a recipient whose STATIC
/// ML-KEM public key is `recip_kem_pub`. Returns the wire page number + env.
fn rt_send(
    st: &mut RatchetState,
    recip_kem_pub: &[u8],
    text: &str,
) -> Result<(u32, u32, HpqEnvelope), String> {
    let (mk, header) = ratchet_step_send(st)?;
    let dhs_pub: [u8; 32] = B64
        .decode(&header.dh)
        .map_err(|e| format!("dh b64: {e}"))?
        .try_into()
        .map_err(|_| "dh size".to_string())?;
    let env = crypto::encrypt_with_mk(text, &mk, recip_kem_pub, &dhs_pub)?;
    Ok((header.pn, header.n, env))
}

/// Open a ratchet envelope into `st` (copy-on-write: `st` is committed ONLY on
/// a successful authenticated decrypt), using our static ML-KEM secret.
fn rt_recv(
    st: &mut RatchetState,
    env: &HpqEnvelope,
    pn: u32,
    n: u32,
    our_kem_secret: &[u8],
) -> Result<String, String> {
    let dh_bytes: [u8; 32] = B64
        .decode(&env.eph)
        .map_err(|e| format!("eph b64: {e}"))?
        .try_into()
        .map_err(|_| "eph size".to_string())?;
    let dh_hex = hx(&dh_bytes);
    let mut clone = st.clone();
    let mk = ratchet_step_recv(&mut clone, &dh_hex, dh_bytes, pn, n)?;
    let kem_ct = B64.decode(&env.kem).map_err(|e| format!("kem b64: {e}"))?;
    let kem_ss = crypto::ml_kem_decapsulate_local(&kem_ct, our_kem_secret)?;
    let pt = crypto::open_with_secrets(env, &mk, &kem_ss)?;
    *st = clone; // commit
    Ok(pt)
}

pub fn self_test_ratchet() -> Result<String, String> {
    // Static identity material for A (inviter/responder) and B (accepter/initiator).
    let (a_x_priv, a_x_pub) = crypto::ratchet_keypair();
    let (a_kem_secret, a_kem_pub) = crypto::generate_ml_kem_keypair();
    let (_b_x_priv, _b_x_pub) = crypto::ratchet_keypair();
    let (b_kem_secret, b_kem_pub) = crypto::generate_ml_kem_keypair();
    // A's published ratchet prekey.
    let (a_rk_priv, a_rk_pub) = crypto::ratchet_keypair();

    // ── Bootstrap. B (initiator) derives SK from a fresh bootstrap ephemeral. ──
    let (b_eph_priv, b_eph_pub) = crypto::ratchet_keypair();
    let x3dh_b = crypto::dh(&b_eph_priv, &a_x_pub);
    let (kem_ct, kem_ss_b) = crypto::ml_kem_encapsulate_local(&a_kem_pub)?;
    let sk_b = crypto::root_init(&x3dh_b, &kem_ss_b);
    let mut state_b = ratchet_init_initiator(sk_b, a_rk_pub);
    let b_dh_pub: [u8; 32] = b32(&state_b.dhs_pub)?;

    // A (responder) recovers SK from its static key + B's bootstrap ephemeral.
    let x3dh_a = crypto::dh(&a_x_priv, &b_eph_pub);
    let kem_ss_a = crypto::ml_kem_decapsulate_local(&kem_ct, &a_kem_secret)?;
    let sk_a = crypto::root_init(&x3dh_a, &kem_ss_a);
    if sk_a != sk_b {
        return Err("bootstrap SK mismatch between initiator and responder".into());
    }
    let mut state_a = ratchet_init_responder(sk_a, a_rk_priv, a_rk_pub, b_dh_pub)?;

    // ── 4-message exchange B→A→B→A, forcing DH turns both ways. ──
    let b_dhs0 = state_b.dhs_priv.clone();
    let (pn, n, env) = rt_send(&mut state_b, &a_kem_pub, "m1")?;
    if rt_recv(&mut state_a, &env, pn, n, &a_kem_secret)? != "m1" {
        return Err("m1 round-trip failed".into());
    }
    let (pn, n, env) = rt_send(&mut state_a, &b_kem_pub, "m2")?;
    if rt_recv(&mut state_b, &env, pn, n, &b_kem_secret)? != "m2" {
        return Err("m2 round-trip failed".into());
    }
    // Receiving m2 must have turned B's DH ratchet — the sending key rotated.
    if state_b.dhs_priv == b_dhs0 {
        return Err("dhs_priv was REUSED across a DH turn (must-fix #5)".into());
    }
    let (pn, n, env3) = rt_send(&mut state_b, &a_kem_pub, "m3")?;
    let (pn4, n4, env4) = rt_send(&mut state_a, &b_kem_pub, "m4")?;
    if rt_recv(&mut state_a, &env3, pn, n, &a_kem_secret)? != "m3" {
        return Err("m3 round-trip failed (forced DH turn)".into());
    }
    if rt_recv(&mut state_b, &env4, pn4, n4, &b_kem_secret)? != "m4" {
        return Err("m4 round-trip failed".into());
    }

    // ── Out-of-order within a chain (≤ MAX_SKIP), across a fresh DH epoch. ──
    let (p5, i5, env5) = rt_send(&mut state_b, &a_kem_pub, "m5")?;
    let (p6, i6, env6) = rt_send(&mut state_b, &a_kem_pub, "m6")?;
    let (p7, i7, env7) = rt_send(&mut state_b, &a_kem_pub, "m7")?;
    if rt_recv(&mut state_a, &env7, p7, i7, &a_kem_secret)? != "m7" {
        return Err("out-of-order m7 (head) failed".into());
    }
    if rt_recv(&mut state_a, &env5, p5, i5, &a_kem_secret)? != "m5" {
        return Err("out-of-order m5 (from skipped) failed".into());
    }
    if rt_recv(&mut state_a, &env6, p6, i6, &a_kem_secret)? != "m6" {
        return Err("out-of-order m6 (from skipped) failed".into());
    }

    // ── Replay of a consumed message must NOT decrypt (old mk deleted). ──
    if rt_recv(&mut state_a, &env7, p7, i7, &a_kem_secret).is_ok() {
        return Err("replay of a consumed message decrypted (mk not deleted)".into());
    }

    // ── Skip caps rejected BEFORE any KDF (must-fix #7) — same-epoch AND the
    //    cross-epoch double-skip (the case the per-call cap used to miss). ──
    {
        // Same-epoch: n beyond nr + MAX_SKIP.
        let mut probe = state_a.clone();
        let dhr = state_a
            .dhr_pub
            .clone()
            .ok_or_else(|| "no dhr for cap probe".to_string())?;
        let dh_bytes = b32(&dhr)?;
        let huge = state_a.nr.saturating_add(MAX_SKIP).saturating_add(5);
        if ratchet_step_recv(&mut probe, &dhr, dh_bytes, 0, huge).is_ok() {
            return Err("same-epoch skip beyond MAX_SKIP was not rejected".into());
        }
        // Cross-epoch: a forged FRESH eph with old-chain pn + new-chain n whose
        // COMBINED work exceeds MAX_SKIP must be rejected (else 2*MAX_SKIP KDFs).
        let mut probe2 = state_a.clone();
        let (_, fake_dh) = crypto::ratchet_keypair();
        let fake_hex = hx(&fake_dh);
        let pn_big = state_a.nr.saturating_add(MAX_SKIP - 100); // old-chain skip ~MAX_SKIP-100
        let n_big = 200; // new-chain skip 200 ⇒ combined > MAX_SKIP
        if ratchet_step_recv(&mut probe2, &fake_hex, fake_dh, pn_big, n_big).is_ok() {
            return Err("cross-epoch combined skip beyond MAX_SKIP was not rejected".into());
        }
    }

    // ── Tampered page number / swapped DH must fail (AEAD authenticates). ──
    let (pt_pn, pt_n, env_t) = rt_send(&mut state_b, &a_kem_pub, "tamper")?;
    if rt_recv(&mut state_a.clone(), &env_t, pt_pn, pt_n.wrapping_add(1), &a_kem_secret).is_ok() {
        return Err("tampered page number decrypted".into());
    }
    let mut env_swapped = env_t.clone();
    let (_, fake_pub) = crypto::ratchet_keypair();
    env_swapped.eph = B64.encode(fake_pub);
    if rt_recv(&mut state_a.clone(), &env_swapped, pt_pn, pt_n, &a_kem_secret).is_ok() {
        return Err("swapped ratchet DH decrypted".into());
    }
    // The untampered original still opens (the failed attempts used clones).
    if rt_recv(&mut state_a, &env_t, pt_pn, pt_n, &a_kem_secret)? != "tamper" {
        return Err("untampered message failed after tamper attempts".into());
    }

    // ── Anonymous contacts decrypt LOCALLY, never via the provider (#3). ──
    let anon = mint_anon_identity()?;
    let anon_contact = DmContact {
        did: anon.did.clone(),
        name: String::new(),
        last_ts: 0,
        last_preview: String::new(),
        unread: 0,
        my_inbound_queue: Some(random_hex(32)),
        my_recv_pseudonym: None,
        their_inbound_queue: None,
        my_send_pseudonym: None,
        peer_pubkeys: None,
        status: ContactStatus::Active,
        mode: IdentityMode::Anonymous,
        anon_identity: Some(anon),
        ratchet_capable: true,
    };
    match decrypt_via_for_contact(&anon_contact)? {
        DecryptVia::Local(_) => {}
        DecryptVia::Provider => {
            return Err("anonymous contact routed to the provider (must-fix #3)".into())
        }
    }

    Ok("✓ ratchet bootstrap + 4-msg DH turns + out-of-order + replay/tamper/cap rejects + anon-local OK".into())
}

// ── Identity wipe ────────────────────────────────────────────────────
//
// Counterpart to session::wipe_identity. Drops every DM artifact:
// contacts list, peer-keys cache, every per-DID conversation file, the
// expiry map, and the outbox. Iterates the contact list FIRST so we
// know which conversation files to delete (storage doesn't expose a
// directory listing).

pub async fn wipe_dm_storage() {
    let contacts = list_contacts().await;
    for c in &contacts {
        let _ = storage::remove(&conv_path(&c.did)).await;
        // Per-contact ratchet state + any not-yet-completed prekey stash.
        remove_ratchet(&c.did).await;
        if let Some(q) = &c.my_inbound_queue {
            remove_ratchet(&format!("pending:{q}")).await;
        }
    }
    let _ = storage::remove(CONTACTS_FILE).await;
    let _ = storage::remove(PEER_KEYS_FILE).await;
    let _ = storage::remove(EXPIRY_FILE).await;
    crate::api::outbox::clear().await;
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

