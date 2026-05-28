// Peer-receive subscription — closes the federation loop.
//
// hey-social's send side is wired up (create_post → ipfs.add_bytes →
// peer.publish post.create.v2), but no one was consuming the events on
// the other side. This module subscribes to the topics we care about and
// routes incoming signed events into local storage updates:
//
//   post.create.v2  → fetch CID from IPFS → decode dag-cbor → write_post
//                     + prepend to feed index
//   post.delete     → remove from feed index + drop the cached post
//   post.react      → update reactions on the matching post
//   post.repost     → update reposts on the matching post
//   post.comment    → append to comments
//   post.comment_react → update reactions on the matching comment
//   post.comment_delete → drop a comment
//   follow.request  → record into notifications + pending followers
//
// Topics we listen on:
//   * hey-v0/user/<our_did>/posts        — events about our own posts
//   * hey-v0/user/<followed_did>/posts   — for each user we follow
//   * hey-v0/follow/<our_did>            — incoming follow requests
//
// Run as a background task started after sign-in (see App::run in lib.rs).
// Polls every POLL_INTERVAL_MS; cheap when no peers, automatically
// scales with topic count.

use std::cell::RefCell;
use std::collections::HashSet;

use serde_json::{json, Value};
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

use crate::api::dms;
use crate::api::groups;
use crate::api::outbox;
use crate::api::posts::{
    materialize_post_from_cid, read_feed_index, read_post, write_feed_index, write_post,
    FeedEntry, Post,
};
use crate::api::profile;
use crate::events::{from_wire_string, verify_signed_event, VerifyResult};
use crate::runtime::{peer, storage};
use crate::session;

const POLL_INTERVAL_MS: i32 = 5_000;
const RECV_LIMIT: u32 = 50;
const NOTIFICATIONS_FILE: &str = "notifications/index.json";

// Session-scoped cache of topics we've already issued join_topic for.
// peer.join_topic is idempotent on the provider side but the round-trip
// is wasteful — with N v2 contacts we used to do N joins every 5s. Now
// we join each topic at most once per page load; on logout the
// thread_local resets (because wasm itself does).
thread_local! {
    static JOINED_TOPICS: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

async fn ensure_joined(topic: &str) {
    let already = JOINED_TOPICS.with(|s| s.borrow().contains(topic));
    if already {
        return;
    }
    if peer::join_topic(topic).await.is_ok() {
        JOINED_TOPICS.with(|s| s.borrow_mut().insert(topic.to_string()));
    }
}

/// Drop a topic from the joined cache + tell the provider to unsubscribe.
/// Used by dms::receive_handshake after queue rotation, when the original
/// invite queue becomes single-use-only.
pub async fn forget_topic(topic: &str) {
    JOINED_TOPICS.with(|s| {
        s.borrow_mut().remove(topic);
    });
    let _ = peer::leave_topic(topic).await;
}

pub async fn run() {
    // Wait until we have a session; the loop is a no-op while signed out.
    loop {
        sleep_ms(POLL_INTERVAL_MS).await;
        if let Some(s) = session::current() {
            if let Err(e) = poll_once(&s.did_key).await {
                web_sys::console::warn_1(&JsValue::from_str(&format!(
                    "[hey-social] peer_receiver poll error: {e}"
                )));
            }
        }
    }
}

async fn poll_once(my_did: &str) -> Result<(), String> {
    let consumer_id = format!("hey-social:{my_did}");

    // 1. Our own posts topic.
    let my_topic = format!("hey-v0/user/{my_did}/posts");
    ensure_joined(&my_topic).await;
    consume_topic(&my_topic, &consumer_id, Some(my_did)).await;

    // 2. For each user we follow, listen on their posts topic.
    let follows = profile::_internal_read_follows().await;
    for did in follows.following.iter() {
        let topic = format!("hey-v0/user/{did}/posts");
        ensure_joined(&topic).await;
        consume_topic(&topic, &consumer_id, Some(my_did)).await;
    }

    // 3. Our follow inbox.
    let follow_topic = format!("hey-v0/follow/{my_did}");
    ensure_joined(&follow_topic).await;
    consume_topic(&follow_topic, &consumer_id, Some(my_did)).await;

    // 4. Legacy DM inbox — back-compat only.
    let dm_topic = format!("hey-v0/dm/{my_did}");
    ensure_joined(&dm_topic).await;
    consume_topic(&dm_topic, &consumer_id, Some(my_did)).await;

    // 5. Metadata-safe per-pair DM queues.
    for (topic, consumer) in dms::my_v2_topics().await {
        ensure_joined(&topic).await;
        consume_v2_queue(&topic, &consumer).await;
    }

    // 6. Outbox flush — retry any sends that failed transiently. Runs
    //    every cycle; the outbox itself applies backoff to each item.
    outbox::flush().await;

    Ok(())
}

/// Pull pending entries from a v2 per-pair queue topic. Entries are
/// NOT SignedEvents — they're `{ type: "dm.v2", envelope }` shapes.
/// We hand each wire string to dms::receive_v2_wire which decrypts +
/// verifies the inner sig.
async fn consume_v2_queue(topic: &str, consumer_id: &str) {
    let args = peer::RecvArgs {
        topic,
        limit: RECV_LIMIT,
        consumer_id,
        // We intentionally do NOT set skip_sender_id here — v2 sender_ids
        // are random per-contact pseudonyms that even we don't track
        // server-side; the inner signature path will drop our own
        // loopback if it ever happens.
        skip_sender_id: None,
    };
    let resp = match peer::recv(args).await {
        Ok(v) => v,
        Err(_) => return,
    };
    let Some(arr) = resp.get("messages").and_then(|m| m.as_array()).cloned() else {
        return;
    };
    for entry in arr {
        let Some(wire) = entry.get("message").and_then(|m| m.as_str()) else {
            continue;
        };
        if let Err(e) = dms::receive_v2_wire(topic, wire).await {
            web_sys::console::warn_1(&JsValue::from_str(&format!(
                "[hey-social] v2 dm consume: {e}"
            )));
        }
    }
}

async fn consume_topic(topic: &str, consumer_id: &str, my_did: Option<&str>) {
    let args = peer::RecvArgs {
        topic,
        limit: RECV_LIMIT,
        consumer_id,
        // Skip our own publish-loopback.
        skip_sender_id: my_did,
    };
    let resp = match peer::recv(args).await {
        Ok(v) => v,
        Err(_) => return,
    };
    let Some(arr) = resp.get("messages").and_then(|m| m.as_array()).cloned() else {
        return;
    };
    for entry in arr {
        // The provider returns each entry as { message, sender_id, ts, ... }
        // where `message` is the wire-string of the signed envelope.
        let Some(wire) = entry.get("message").and_then(|m| m.as_str()) else {
            continue;
        };
        let Some(evt) = from_wire_string(wire) else {
            continue;
        };
        if verify_signed_event(&evt) != VerifyResult::Valid {
            continue;
        }
        if let Err(e) = route(&evt.event_type, &evt.payload, &evt.sender_did).await {
            web_sys::console::warn_1(&JsValue::from_str(&format!(
                "[hey-social] route {}: {e}",
                evt.event_type
            )));
        }
    }
}

async fn route(event_type: &str, payload: &Value, sender_did: &str) -> Result<(), String> {
    match event_type {
        "post.create.v2" => {
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
            // Sanity: refuse if sender_did doesn't match the body's author.
            if post.user_did != sender_did {
                return Ok(());
            }
            // Stamp a fresh local id; assign post_cid (already set by
            // materialize_post_from_cid, but be defensive).
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
        }
        "post.delete" => {
            let id = payload
                .get("post_id")
                .and_then(|c| c.as_str())
                .ok_or_else(|| "post.delete missing post_id".to_string())?;
            let idx = read_feed_index().await.map_err(|e| e.to_string())?;
            let filtered: Vec<_> = idx.into_iter().filter(|e| e.id != id).collect();
            let _ = storage::remove(&format!("posts/by-id/{id}.json")).await;
            write_feed_index(&filtered).await.map_err(|e| e.to_string())?;
        }
        "post.react" => {
            let post_id = payload.get("post_id").and_then(|c| c.as_str());
            let emoji = payload.get("emoji").and_then(|c| c.as_str());
            let reactor = payload.get("reactor_did").and_then(|c| c.as_str());
            let (Some(post_id), Some(emoji), Some(reactor)) = (post_id, emoji, reactor) else {
                return Ok(());
            };
            apply_react_overlay(post_id, emoji, reactor).await;
        }
        "post.comment" => {
            let post_id = payload.get("post_id").and_then(|c| c.as_str());
            let comment = payload.get("comment");
            let (Some(post_id), Some(comment)) = (post_id, comment) else {
                return Ok(());
            };
            apply_comment_overlay(post_id, comment.clone()).await;
        }
        "follow.request" => {
            // Drop a notification + bump pending followers list.
            push_notification(json!({
                "id": uuid::Uuid::new_v4().to_string(),
                "type": "follow.request",
                "from_did": sender_did,
                "from_name": payload.get("from_name").and_then(|n| n.as_str()).unwrap_or(""),
                "ts": payload.get("ts").cloned().unwrap_or(Value::Null),
                "read": false,
            }))
            .await;
        }
        "dm.message" => {
            let _ = dms::receive_message(sender_did, payload).await;
            push_notification(json!({
                "id": uuid::Uuid::new_v4().to_string(),
                "type": "dm.message",
                "from_did": sender_did,
                "from_name": "",
                "ts": payload.get("ts").cloned().unwrap_or(Value::Null),
                "read": false,
            }))
            .await;
        }
        "group.create.v1" | "group.message.v1" => {
            let _ = groups::receive_event(sender_did, event_type, payload).await;
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
        }
        _ => { /* other event types ignored for now */ }
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
    let _ = post;
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
    // Trim to 100 most-recent.
    notes.truncate(100);
    let _ = storage::write_json(NOTIFICATIONS_FILE, &json!({ "notifications": notes })).await;
}

async fn sleep_ms(ms: i32) {
    let win = web_sys::window().unwrap();
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let _ = win
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
    });
    let _ = JsFuture::from(promise).await;
}

// Make Post Debug usable so we can dump in console on error paths. No-op
// outside that path.
#[allow(dead_code)]
fn debug_post(p: &Post) {
    web_sys::console::log_1(&JsValue::from_str(&format!("{p:?}")));
}
