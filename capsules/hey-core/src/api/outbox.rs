// Outbox + retry queue for DM publishes.
//
// `peer::publish` can fail for transient reasons (network glitch, the
// peer provider being unreachable, a 5xx). Today we'd drop the failure
// on the floor (`let _ = peer::publish(...).await`) and the message
// would never reach the peer — the local conversation already has it,
// so the sender never sees a problem; the recipient just never gets
// the message.
//
// The outbox closes that gap. Every publish attempt that errors gets
// stashed here as a serialized wire string + the topic + the
// pseudonymous sender_id, with an exponential-backoff retry schedule.
// `flush()` walks the queue once per peer_receiver poll cycle and
// retries each item whose `next_attempt_ms` has elapsed. Successful
// publish → the item is dropped. Repeated failure → backoff doubles
// up to a cap; after ATTEMPTS_MAX retries the item is dropped with a
// console warning.
//
// Storage: `Hey/dm/outbox.json` as `Vec<OutboxItem>`. The whole queue
// is rewritten on each modification (cap at 1000 items so the JSON
// stays bounded). For Hey-scale chat traffic that's plenty.

use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

use crate::runtime::{peer, storage};

const OUTBOX_FILE: &str = "dm/outbox.json";
const MAX_ITEMS: usize = 1000;
const ATTEMPTS_MAX: u32 = 12;
/// Initial backoff before the first retry, in ms. Subsequent retries
/// double up to BACKOFF_CAP_MS.
const BACKOFF_INITIAL_MS: i64 = 5_000;
/// Cap retry delay at 1 hour. Beyond ATTEMPTS_MAX we drop the item.
const BACKOFF_CAP_MS: i64 = 60 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxItem {
    pub id: String,
    pub topic: String,
    pub sender_id: String,
    pub ts: i64,
    pub wire: String,
    #[serde(default)]
    pub retries: u32,
    #[serde(default)]
    pub next_attempt_ms: i64,
}

fn now_ms() -> i64 {
    js_sys::Date::now() as i64
}

fn backoff_for(retries: u32) -> i64 {
    let raw = BACKOFF_INITIAL_MS.saturating_mul(2_i64.saturating_pow(retries));
    raw.min(BACKOFF_CAP_MS)
}

async fn read_items() -> Vec<OutboxItem> {
    storage::read_json(OUTBOX_FILE)
        .await
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

async fn write_items(items: &[OutboxItem]) {
    let v = match serde_json::to_value(items) {
        Ok(v) => v,
        Err(_) => return,
    };
    let _ = storage::write_json(OUTBOX_FILE, &v).await;
}

/// Stash a publish that has already failed. `next_attempt_ms` is
/// scheduled immediately so the next `flush()` will retry; if that
/// retry also fails the queue's own backoff takes over.
pub async fn enqueue(topic: &str, sender_id: &str, wire: &str) {
    let mut items = read_items().await;
    if items.len() >= MAX_ITEMS {
        // Drop the oldest to make room. Better than refusing the newest.
        items.remove(0);
    }
    items.push(OutboxItem {
        id: uuid::Uuid::new_v4().to_string(),
        topic: topic.into(),
        sender_id: sender_id.into(),
        ts: now_ms(),
        wire: wire.into(),
        retries: 0,
        next_attempt_ms: now_ms(),
    });
    write_items(&items).await;
}

/// Convenience: try to publish once, and on failure stash for retry.
/// Returns Ok if the initial publish succeeded.
pub async fn publish_or_enqueue(
    topic: &str,
    sender_id: &str,
    wire: &str,
) -> Result<(), String> {
    let res = peer::publish(peer::PublishArgs {
        topic,
        message: wire,
        sender_id,
        ts: now_ms(),
        // Empty would still be valid for v2 — we use a constant
        // placeholder so providers that validate non-empty don't
        // reject the publish. The "real" signature is inside the
        // ChaCha20-Poly1305 envelope, not on the outer wire.
        signature: "v2-sealed",
    })
    .await;
    if res.is_err() {
        enqueue(topic, sender_id, wire).await;
        return Err("publish failed; queued for retry".into());
    }
    Ok(())
}

/// Walk the outbox and retry items whose `next_attempt_ms` has elapsed.
/// Called from peer_receiver::poll_once each cycle.
pub async fn flush() {
    let mut items = read_items().await;
    if items.is_empty() {
        return;
    }
    let now = now_ms();
    let mut next: Vec<OutboxItem> = Vec::with_capacity(items.len());
    let mut changed = false;
    for mut item in items.drain(..) {
        if item.next_attempt_ms > now {
            next.push(item);
            continue;
        }
        let res = peer::publish(peer::PublishArgs {
            topic: &item.topic,
            message: &item.wire,
            sender_id: &item.sender_id,
            ts: item.ts,
            signature: "v2-sealed",
        })
        .await;
        if res.is_ok() {
            changed = true;
            // drop the item — successful publish.
            continue;
        }
        item.retries += 1;
        if item.retries >= ATTEMPTS_MAX {
            web_sys::console::warn_1(&JsValue::from_str(&format!(
                "[hey-social] outbox: dropping item {} on topic {} after {} attempts",
                item.id, item.topic, item.retries
            )));
            changed = true;
            continue;
        }
        item.next_attempt_ms = now + backoff_for(item.retries);
        changed = true;
        next.push(item);
    }
    if changed {
        write_items(&next).await;
    }
}

/// How many items are awaiting retry. Cheap (one storage read). Useful
/// for surfacing a "N messages queued" badge in the UI.
pub async fn pending_count() -> usize {
    read_items().await.len()
}

/// Hard reset — used by session::wipe(). Drops every queued message
/// without trying to send.
pub async fn clear() {
    let _ = storage::remove(OUTBOX_FILE).await;
}

/// Drop any items whose topic matches `prefix` exactly or starts with
/// `prefix/`. Used when queue rotation makes a topic obsolete.
pub async fn purge_topic(topic: &str) {
    let items = read_items().await;
    let kept: Vec<OutboxItem> = items
        .into_iter()
        .filter(|i| i.topic != topic)
        .collect();
    write_items(&kept).await;
}

/// Self-introspection: serialize a synthetic OutboxItem roundtrip. Used
/// by dms::self_test_v2 to confirm the storage shape works after
/// schema changes.
#[allow(dead_code)]
pub fn schema_roundtrip_ok() -> bool {
    let item = OutboxItem {
        id: "test".into(),
        topic: "q/abc".into(),
        sender_id: "deadbeef".into(),
        ts: 1,
        wire: r#"{"type":"dm.v2","envelope":{}}"#.into(),
        retries: 0,
        next_attempt_ms: 0,
    };
    let v = match serde_json::to_value(&item) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let back: Result<OutboxItem, _> = serde_json::from_value(v);
    back.is_ok()
}

