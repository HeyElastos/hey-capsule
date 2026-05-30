// hey-social's federation handlers — registered into the SHARED engine receiver.
//
// The poll loop, topic subscription bookkeeping, SignedEvent verification, DM
// storage, and v2-queue draining all live in `hey_core::peer_receiver` now (one
// implementation shared with hey-chat). hey-social keeps only its DOMAIN arms:
// it registers handlers for its own event types (post.* / follow.request /
// group.* / a DM-notification rider) and provides the extra topics it must drain
// each poll (its own + followed-user post topics + its follow inbox). Boot calls
// `register()` then `hey_core::peer_receiver::run()`.
//
//   post.create.v2  → fetch CID from IPFS → decode dag-cbor → write_post + feed
//   post.delete     → remove from feed index + drop the cached post
//   post.react      → toggle reaction overlay
//   post.comment    → append comment overlay
//   follow.request  → notification + pending follower
//   dm.message      → (engine already stored it) raise a notification
//   group.*         → groups::receive_event + notification

use serde_json::{json, Value};

use crate::api::groups;
use crate::api::posts::{
    materialize_post_from_cid, read_feed_index, read_post, write_feed_index, write_post, FeedEntry,
};
use crate::api::profile;
use crate::runtime::storage;
use crate::session;

const NOTIFICATIONS_FILE: &str = "notifications/index.json";

/// Wire hey-social's domain handlers + extra topics into the shared engine
/// receiver. MUST run before `hey_core::peer_receiver::run()`.
pub fn register() {
    use hey_core::peer_receiver::{register_handler, set_extra_topics_provider};
    register_handler("post.create.v2", |_t, payload, sender| handle_post_create(payload, sender));
    register_handler("post.delete", |_t, payload, _s| handle_post_delete(payload));
    register_handler("post.react", |_t, payload, _s| handle_post_react(payload));
    register_handler("post.comment", |_t, payload, _s| handle_post_comment(payload));
    register_handler("follow.request", |_t, payload, sender| handle_follow_request(payload, sender));
    // The engine stores the DM itself; we only add the notification.
    register_handler("dm.message", |_t, payload, sender| handle_dm_notify(payload, sender));
    register_handler("group.create.v1", |t, payload, sender| handle_group(t, payload, sender));
    register_handler("group.message.v1", |t, payload, sender| handle_group(t, payload, sender));
    set_extra_topics_provider(extra_topics);
}

/// Topics hey-social must subscribe + drain each poll, beyond the engine's DM
/// topics: our own posts topic, each followed user's posts topic, our follow
/// inbox. All carry SignedEvents → routed to the handlers above.
async fn extra_topics() -> Vec<String> {
    let Some(s) = session::current() else {
        return Vec::new();
    };
    let my_did = s.did_key;
    if my_did.is_empty() {
        return Vec::new();
    }
    let mut topics = vec![format!("hey-v0/user/{my_did}/posts")];
    let follows = profile::_internal_read_follows().await;
    for did in follows.following.iter() {
        topics.push(format!("hey-v0/user/{did}/posts"));
    }
    topics.push(format!("hey-v0/follow/{my_did}"));
    topics
}

// ── Handlers (owned args; the engine clones before dispatch) ──────────

async fn handle_post_create(payload: Value, sender_did: String) -> Result<(), String> {
    let cid = payload
        .get("post_cid")
        .and_then(|c| c.as_str())
        .ok_or_else(|| "post.create.v2 missing post_cid".to_string())?;
    // Bail out if we already have this CID locally.
    let idx = read_feed_index().await.map_err(|e| e.to_string())?;
    if idx.iter().any(|e| e.post_cid.as_deref() == Some(cid)) {
        return Ok(());
    }
    let Some(mut post) = materialize_post_from_cid(cid).await else {
        return Ok(());
    };
    // Refuse if sender_did doesn't match the body's author.
    if post.user_did != sender_did {
        return Ok(());
    }
    post.post_cid = Some(cid.to_string());
    let entry = FeedEntry {
        id: post.id.clone(),
        ts: post.ts,
        author: post.user_did.clone(),
        post_cid: Some(cid.to_string()),
    };
    let _ = write_post(&post).await;
    let mut idx = read_feed_index().await.map_err(|e| e.to_string())?;
    idx.insert(0, entry);
    write_feed_index(&idx).await.map_err(|e| e.to_string())?;
    Ok(())
}

async fn handle_post_delete(payload: Value) -> Result<(), String> {
    let id = payload
        .get("post_id")
        .and_then(|c| c.as_str())
        .ok_or_else(|| "post.delete missing post_id".to_string())?;
    let idx = read_feed_index().await.map_err(|e| e.to_string())?;
    let filtered: Vec<_> = idx.into_iter().filter(|e| e.id != id).collect();
    let _ = storage::remove(&format!("posts/by-id/{id}.json")).await;
    write_feed_index(&filtered).await.map_err(|e| e.to_string())?;
    Ok(())
}

async fn handle_post_react(payload: Value) -> Result<(), String> {
    let post_id = payload.get("post_id").and_then(|c| c.as_str());
    let emoji = payload.get("emoji").and_then(|c| c.as_str());
    let reactor = payload.get("reactor_did").and_then(|c| c.as_str());
    if let (Some(post_id), Some(emoji), Some(reactor)) = (post_id, emoji, reactor) {
        apply_react_overlay(post_id, emoji, reactor).await;
    }
    Ok(())
}

async fn handle_post_comment(payload: Value) -> Result<(), String> {
    let post_id = payload.get("post_id").and_then(|c| c.as_str());
    let comment = payload.get("comment");
    if let (Some(post_id), Some(comment)) = (post_id, comment) {
        apply_comment_overlay(post_id, comment.clone()).await;
    }
    Ok(())
}

async fn handle_follow_request(payload: Value, sender_did: String) -> Result<(), String> {
    push_notification(json!({
        "id": uuid::Uuid::new_v4().to_string(),
        "type": "follow.request",
        "from_did": sender_did,
        "from_name": payload.get("from_name").and_then(|n| n.as_str()).unwrap_or(""),
        "ts": payload.get("ts").cloned().unwrap_or(Value::Null),
        "read": false,
    }))
    .await;
    Ok(())
}

async fn handle_dm_notify(payload: Value, sender_did: String) -> Result<(), String> {
    push_notification(json!({
        "id": uuid::Uuid::new_v4().to_string(),
        "type": "dm.message",
        "from_did": sender_did,
        "from_name": "",
        "ts": payload.get("ts").cloned().unwrap_or(Value::Null),
        "read": false,
    }))
    .await;
    Ok(())
}

async fn handle_group(event_type: String, payload: Value, sender_did: String) -> Result<(), String> {
    let _ = groups::receive_event(&sender_did, &event_type, &payload).await;
    if event_type == "group.message.v1" {
        push_notification(json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "type": "group.message",
            "from_did": sender_did,
            "from_name": payload.get("sender_name").and_then(|v| v.as_str()).unwrap_or(""),
            "ts": payload.get("ts").cloned().unwrap_or(Value::Null),
            "read": false,
        }))
        .await;
    }
    Ok(())
}

async fn apply_react_overlay(post_id: &str, emoji: &str, reactor: &str) {
    let Ok(Some(mut post)) = read_post(post_id).await else {
        return;
    };
    let mut reactions = post.reactions.clone();
    let entry = reactions.entry(emoji.to_string()).or_insert(json!([]));
    let Some(list) = entry.as_array_mut() else {
        return;
    };
    if let Some(pos) = list.iter().position(|v| v.as_str() == Some(reactor)) {
        list.remove(pos);
    } else {
        list.push(json!(reactor));
    }
    if list.is_empty() {
        reactions.remove(emoji);
    }
    post.reactions = reactions;
    let _ = write_post(&post).await;
}

async fn apply_comment_overlay(post_id: &str, comment_value: Value) {
    let Ok(Some(mut post)) = read_post(post_id).await else {
        return;
    };
    if let Ok(c) = serde_json::from_value(comment_value) {
        post.comments.push(c);
        let _ = write_post(&post).await;
    }
}

async fn push_notification(n: Value) {
    let wrap = storage::read_json(NOTIFICATIONS_FILE)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| json!({ "notifications": [] }));
    let mut notes = wrap
        .get("notifications")
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
    notes.insert(0, n);
    notes.truncate(100);
    let _ = storage::write_json(NOTIFICATIONS_FILE, &json!({ "notifications": notes })).await;
}
