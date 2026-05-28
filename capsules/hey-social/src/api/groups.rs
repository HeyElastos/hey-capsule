// Group chat — N-to-N envelope fanout.
//
// Design (deliberately simpler than MLS for v1):
//   * A Group is a local record on the creator's node: id, name,
//     members, creator_did, created_at + last_ts/last_preview/unread
//     for the contact-list affordance.
//   * Send: encrypt the message ONCE PER MEMBER using the existing
//     PQ-hybrid envelope (crypto::encrypt_to_hybrid), publish each
//     copy on the member's DM topic with a `group.message.v1` event
//     type and a payload carrying { group_id, group_name, members,
//     envelope|text }.
//   * Receive: peer_receiver routes group.message.v1 → decrypts our
//     envelope copy or accepts the plaintext bootstrap → stores in
//     the per-group conversation. Recipients materialize the group
//     from the included metadata on first receive (no separate
//     "join" handshake).
//   * Member changes: the sender includes the current member list in
//     every group.message.v1. Recipients adopt the union as their
//     membership view (eventual consistency).
//
// Trade-offs vs MLS:
//   * No formal add/remove operation ordering — last-write-wins on
//     membership.
//   * No epoch-keyed forward secrecy at the group level (each pair
//     still has its own per-message FS via X25519 ephemeral keys).
//   * No tree-based scaling — broadcast is O(N) per message.
//   * Group rekeying on member removal isn't atomic.
//
// Acceptable for tens-of-members chats today; revisit with openmls
// when we need hundreds.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::api::dms::{cache_peer_keys, get_peer_keys, PeerKeys};
use crate::api::profile::ensure_profile;
use crate::crypto::{self, HpqEnvelope};
use crate::events::create_signed_event;
use crate::identity::hex_to_bytes;
use crate::runtime::{peer, storage, RuntimeError};
use crate::session;

const GROUPS_INDEX: &str = "groups/index.json";

fn group_msgs_path(group_id: &str) -> String {
    let safe = group_id.replace(['/', ':'], "_");
    format!("groups/by-id/{safe}.json")
}

fn now_ms() -> i64 {
    js_sys::Date::now() as i64
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub members: Vec<String>,
    #[serde(default, rename = "creatorDid")]
    pub creator_did: String,
    #[serde(default, rename = "createdAt")]
    pub created_at: i64,
    #[serde(default, rename = "lastTs")]
    pub last_ts: i64,
    #[serde(default, rename = "lastPreview")]
    pub last_preview: String,
    #[serde(default)]
    pub unread: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMessage {
    pub id: String,
    #[serde(rename = "groupId")]
    pub group_id: String,
    pub sender_did: String,
    #[serde(default)]
    pub sender_name: String,
    pub text: String,
    pub ts: i64,
    pub mine: bool,
    #[serde(default)]
    pub encrypted: bool,
}

pub async fn list_groups() -> Vec<Group> {
    storage::read_json(GROUPS_INDEX)
        .await
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

async fn write_groups(list: &[Group]) -> Result<(), RuntimeError> {
    let v = serde_json::to_value(list)
        .map_err(|e| RuntimeError::new(format!("serialize: {e}")))?;
    storage::write_json(GROUPS_INDEX, &v).await
}

pub async fn read_group(group_id: &str) -> Option<Group> {
    list_groups().await.into_iter().find(|g| g.id == group_id)
}

pub async fn read_messages(group_id: &str) -> Vec<GroupMessage> {
    storage::read_json(&group_msgs_path(group_id))
        .await
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

async fn write_messages(group_id: &str, msgs: &[GroupMessage]) -> Result<(), RuntimeError> {
    let v = serde_json::to_value(msgs).map_err(|e| RuntimeError::new(format!("serialize: {e}")))?;
    storage::write_json(&group_msgs_path(group_id), &v).await
}

pub async fn mark_read(group_id: &str) {
    let mut list = list_groups().await;
    if let Some(g) = list.iter_mut().find(|g| g.id == group_id) {
        g.unread = 0;
        let _ = write_groups(&list).await;
    }
}

async fn upsert_group(updated: Group) {
    let mut list = list_groups().await;
    if let Some(g) = list.iter_mut().find(|g| g.id == updated.id) {
        g.name = updated.name;
        // Adopt the union of member lists for eventual consistency.
        for m in &updated.members {
            if !g.members.contains(m) {
                g.members.push(m.clone());
            }
        }
        if updated.last_ts > g.last_ts {
            g.last_ts = updated.last_ts;
            g.last_preview = updated.last_preview;
        }
        g.unread = g.unread.saturating_add(updated.unread);
    } else {
        list.push(updated);
    }
    list.sort_by(|a, b| b.last_ts.cmp(&a.last_ts));
    let _ = write_groups(&list).await;
}

/// Create a new group, broadcast a creation marker so members see it
/// in their group list immediately. Returns the Group.
pub async fn create_group(name: &str, members: Vec<String>) -> Result<Group, String> {
    let me = ensure_profile().await.map_err(|e| e.to_string())?;
    let mut all_members: Vec<String> = members
        .into_iter()
        .filter(|m| m.starts_with("did:key:z") && m != &me.did_key)
        .collect();
    all_members.sort();
    all_members.dedup();
    if all_members.is_empty() {
        return Err("Add at least one member.".into());
    }

    let group = Group {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.trim().chars().take(60).collect::<String>(),
        members: all_members,
        creator_did: me.did_key.clone(),
        created_at: now_ms(),
        last_ts: now_ms(),
        last_preview: "Group created".into(),
        unread: 0,
    };
    let mut list = list_groups().await;
    list.insert(0, group.clone());
    let _ = write_groups(&list).await;

    // Send a creation marker so other members materialize the group
    // even before the first real message.
    let _ = send_group_event(&group, "Group created", "group.create.v1").await;
    Ok(group)
}

fn my_public_pubkeys() -> Option<PeerKeys> {
    let s = session::current()?;
    if s.ml_kem_public_b64.is_empty() {
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

// Build + sign + publish a group event to every member except self.
// For each recipient: if we have their pubkeys cached we wrap the
// text in a hybrid PQ envelope; otherwise we fall back to plaintext
// bootstrap (same pattern as 1:1 DMs).
async fn send_group_event(group: &Group, text: &str, event_type: &str) -> Result<(), String> {
    let me = ensure_profile().await.map_err(|e| e.to_string())?;
    let s = session::current().ok_or_else(|| "not signed in".to_string())?;
    let my_pub = my_public_pubkeys();

    for member_did in &group.members {
        if member_did == &me.did_key {
            continue;
        }
        let peer_keys = get_peer_keys(member_did).await;
        let payload = if let Some(pk) = peer_keys {
            let recipient_x25519: [u8; 32] = match B64.decode(&pk.x25519_pub_b64) {
                Ok(b) => match b.try_into() {
                    Ok(arr) => arr,
                    Err(_) => continue,
                },
                Err(_) => continue,
            };
            let recipient_kem = match B64.decode(&pk.ml_kem_pub_b64) {
                Ok(b) => b,
                Err(_) => continue,
            };
            match crypto::encrypt_to_hybrid(text, &recipient_x25519, &recipient_kem) {
                Ok(env) => json!({
                    "group_id": group.id,
                    "group_name": group.name,
                    "members": group.members,
                    "sender_name": me.name,
                    "sender_pubkeys": my_pub,
                    "envelope": env,
                    "ts": now_ms(),
                }),
                Err(_) => continue,
            }
        } else {
            json!({
                "group_id": group.id,
                "group_name": group.name,
                "members": group.members,
                "sender_name": me.name,
                "sender_pubkeys": my_pub,
                "text": text,
                "ts": now_ms(),
                "bootstrap": true,
            })
        };

        let evt = match create_signed_event(event_type, payload, &s.auth_key_hex) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let wire = crate::events::to_wire_string(&evt);
        let topic = format!("hey-v0/dm/{member_did}");
        let _ = peer::join_topic(&topic).await;
        let _ = peer::publish(peer::PublishArgs {
            topic: &topic,
            message: &wire,
            sender_id: &evt.sender_did,
            ts: evt.ts,
            signature: &evt.signature,
        })
        .await;
    }
    Ok(())
}

pub async fn send_message(group_id: &str, text: &str) -> Result<GroupMessage, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("empty message".into());
    }
    let me = ensure_profile().await.map_err(|e| e.to_string())?;
    let group = read_group(group_id)
        .await
        .ok_or_else(|| "group not found".to_string())?;
    let body: String = trimmed.chars().take(4096).collect();

    let msg = GroupMessage {
        id: uuid::Uuid::new_v4().to_string(),
        group_id: group_id.into(),
        sender_did: me.did_key.clone(),
        sender_name: me.name.clone(),
        text: body.clone(),
        ts: now_ms(),
        mine: true,
        encrypted: true, // best-effort; per-recipient envelope may still bootstrap
    };
    let mut msgs = read_messages(group_id).await;
    msgs.push(msg.clone());
    write_messages(group_id, &msgs)
        .await
        .map_err(|e| e.to_string())?;

    // Bump our local group lastTs/preview.
    let mut list = list_groups().await;
    if let Some(g) = list.iter_mut().find(|g| g.id == group_id) {
        g.last_ts = msg.ts;
        g.last_preview = msg.text.chars().take(140).collect();
        let _ = write_groups(&list).await;
    }

    send_group_event(&group, &body, "group.message.v1").await?;
    Ok(msg)
}

/// Receive a group event (called by peer_receiver). Caller has already
/// verified the Ed25519 signature. Materializes the group on first
/// receive, decrypts the envelope or accepts plaintext bootstrap, and
/// stores in the per-group conversation.
pub async fn receive_event(
    sender_did: &str,
    event_type: &str,
    payload: &Value,
) -> Result<(), String> {
    let me = ensure_profile().await.map_err(|e| e.to_string())?;
    if sender_did == me.did_key {
        return Ok(());
    }

    // Cache sender's pubkeys if present (used for replies).
    if let Some(pk) = payload.get("sender_pubkeys") {
        if let Ok(parsed) = serde_json::from_value::<PeerKeys>(pk.clone()) {
            cache_peer_keys(sender_did, parsed).await;
        }
    }

    let group_id = payload
        .get("group_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "group.* missing group_id".to_string())?
        .to_string();
    let group_name = payload
        .get("group_name")
        .and_then(|v| v.as_str())
        .unwrap_or("Group")
        .to_string();
    let mut members: Vec<String> = payload
        .get("members")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|m| m.as_str().map(String::from)).collect())
        .unwrap_or_default();
    // Always include the sender + ourselves in the implied membership.
    if !members.contains(&sender_did.to_string()) {
        members.push(sender_did.into());
    }
    if !members.contains(&me.did_key) {
        members.push(me.did_key.clone());
    }
    members.sort();
    members.dedup();

    // Materialize / update group locally.
    let sender_name = payload
        .get("sender_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let ts = payload
        .get("ts")
        .and_then(|v| v.as_i64())
        .unwrap_or_else(now_ms);
    let preview = if event_type == "group.create.v1" {
        format!("{sender_name} created the group")
    } else {
        payload
            .get("text")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_default()
    };
    upsert_group(Group {
        id: group_id.clone(),
        name: group_name,
        members,
        creator_did: sender_did.into(),
        created_at: ts,
        last_ts: ts,
        last_preview: preview.chars().take(140).collect(),
        unread: if event_type == "group.message.v1" { 1 } else { 0 },
    })
    .await;

    if event_type == "group.create.v1" {
        return Ok(());
    }

    // group.message.v1 — decrypt or bootstrap.
    let (text, encrypted) = if let Some(env_val) = payload.get("envelope") {
        let env: HpqEnvelope = serde_json::from_value(env_val.clone())
            .map_err(|e| format!("envelope shape: {e}"))?;
        let my_keys = load_my_keys()?;
        let pt = crypto::decrypt_hybrid(&env, &my_keys)?;
        (pt, true)
    } else if let Some(t) = payload.get("text").and_then(|v| v.as_str()) {
        (t.to_string(), false)
    } else {
        return Err("group.message.v1 has neither envelope nor text".into());
    };

    let msg = GroupMessage {
        id: uuid::Uuid::new_v4().to_string(),
        group_id: group_id.clone(),
        sender_did: sender_did.into(),
        sender_name,
        text: text.chars().take(4096).collect(),
        ts,
        mine: false,
        encrypted,
    };
    let mut msgs = read_messages(&group_id).await;
    msgs.push(msg);
    write_messages(&group_id, &msgs)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn load_my_keys() -> Result<crypto::UserKeys, String> {
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
