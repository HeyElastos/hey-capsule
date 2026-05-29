// Peer-receive loop for the shared chat engine — the DM half of
// hey-social's peer_receiver, with the social routing stripped.
//
// hey-social's version also routed post.create.v2 / post.* / follow.request /
// group.* and read the follows store to subscribe to followed users' post
// topics. None of that belongs in the messenger, so it is gone here. What
// remains is the chat loop: subscribe to the legacy DM inbox + the
// metadata-safe v2 per-pair queues, route incoming DM events into the DM
// store, and flush the outbox each cycle.
//
// Topics:
//   * hey-v0/dm/<our_did>        — legacy v1 DM inbox (back-compat)
//   * q/<256bit> (per-pair)      — v2 sealed-sender queues (dms::my_v2_topics)
//
// Run as a background task started after sign-in (see the bin crate's boot).
// When hey-social adopts hey-core, route() can be made pluggable to re-add
// its social arms.

use std::cell::RefCell;
use std::collections::HashSet;

use serde_json::Value;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

use crate::api::dms;
use crate::api::outbox;
use crate::events::{from_wire_string, verify_signed_event, VerifyResult};
use crate::runtime::peer;
use crate::session;

const POLL_INTERVAL_MS: i32 = 5_000;
const RECV_LIMIT: u32 = 50;

// Session-scoped cache of topics we've already issued join_topic for —
// join is idempotent provider-side but the round-trip is wasteful. Resets
// on logout (wasm itself resets).
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
/// Used by dms::receive_handshake after queue rotation makes an invite
/// queue single-use.
pub async fn forget_topic(topic: &str) {
    JOINED_TOPICS.with(|s| {
        s.borrow_mut().remove(topic);
    });
    let _ = peer::leave_topic(topic).await;
}

/// Background poll loop. No-op while signed out.
pub async fn run() {
    loop {
        sleep_ms(POLL_INTERVAL_MS).await;
        if let Some(s) = session::current() {
            if let Err(e) = poll_once(&s.did_key).await {
                web_sys::console::warn_1(&JsValue::from_str(&format!(
                    "[hey-core] peer_receiver poll error: {e}"
                )));
            }
        }
    }
}

async fn poll_once(my_did: &str) -> Result<(), String> {
    let consumer_id = format!("{}:{}", crate::ctx::capsule_id(), my_did);

    // 1. Legacy DM inbox — back-compat with v1 hey-v0/dm/<did> senders.
    let dm_topic = format!("hey-v0/dm/{my_did}");
    ensure_joined(&dm_topic).await;
    consume_topic(&dm_topic, &consumer_id, Some(my_did)).await;

    // 2. Metadata-safe per-pair v2 DM queues.
    for (topic, consumer) in dms::my_v2_topics().await {
        ensure_joined(&topic).await;
        consume_v2_queue(&topic, &consumer).await;
    }

    // 3. Retry any sends that failed transiently.
    outbox::flush().await;

    Ok(())
}

/// Pull pending entries from a v2 per-pair queue. Entries are
/// `{ type: "dm.v2", envelope }` (NOT SignedEvents) — hand each wire string
/// to dms::receive_v2_wire which decrypts + verifies the inner sig.
async fn consume_v2_queue(topic: &str, consumer_id: &str) {
    let args = peer::RecvArgs {
        topic,
        limit: RECV_LIMIT,
        consumer_id,
        // v2 sender_ids are random per-contact pseudonyms; the inner sig
        // path drops our own loopback if it ever happens.
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
        // peer v1.1 body field is `content`; older builds used `message`.
        let Some(wire) = entry
            .get("content")
            .or_else(|| entry.get("message"))
            .and_then(|m| m.as_str())
        else {
            continue;
        };
        if let Err(e) = dms::receive_v2_wire(topic, wire).await {
            web_sys::console::warn_1(&JsValue::from_str(&format!("[hey-core] v2 dm consume: {e}")));
        }
    }
}

/// Pull SignedEvent entries from a plain topic (the legacy DM inbox).
async fn consume_topic(topic: &str, consumer_id: &str, my_did: Option<&str>) {
    let args = peer::RecvArgs {
        topic,
        limit: RECV_LIMIT,
        consumer_id,
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
        let Some(wire) = entry
            .get("content")
            .or_else(|| entry.get("message"))
            .and_then(|m| m.as_str())
        else {
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
                "[hey-core] route {}: {e}",
                evt.event_type
            )));
        }
    }
}

/// DM-only routing. Non-DM event types are ignored by the messenger.
async fn route(event_type: &str, payload: &Value, sender_did: &str) -> Result<(), String> {
    if event_type == "dm.message" {
        let _ = dms::receive_message(sender_did, payload).await;
    }
    Ok(())
}

async fn sleep_ms(ms: i32) {
    let win = web_sys::window().unwrap();
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
    });
    let _ = JsFuture::from(promise).await;
}
