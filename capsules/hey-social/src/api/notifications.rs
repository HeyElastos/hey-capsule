// Notifications storage helpers.
//
// Layout: `Hey/notifications/index.json` =
//   { notifications: [{ id, type, from_did, from_name, ts, read, ... }, ...] }
//
// The peer_receiver appends to this on incoming follow.request /
// post.react / etc. events. The bell + NotificationPanel in TopHeader
// reads + marks-as-read via the helpers here.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::runtime::{storage, RuntimeError};

pub const NOTIFICATIONS_FILE: &str = "notifications/index.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Notification {
    pub id: String,
    #[serde(rename = "type", default)]
    pub event_type: String,
    #[serde(default)]
    pub from_did: String,
    #[serde(default)]
    pub from_name: String,
    #[serde(default)]
    pub ts: Option<i64>,
    #[serde(default)]
    pub read: bool,
    #[serde(default)]
    pub post_id: Option<String>,
    #[serde(default)]
    pub emoji: Option<String>,
}

pub async fn list() -> Vec<Notification> {
    let v = storage::read_json(NOTIFICATIONS_FILE).await.ok().flatten();
    let Some(v) = v else { return Vec::new() };
    let arr = v
        .get("notifications")
        .and_then(|a| a.as_array())
        .cloned()
        .unwrap_or_default();
    arr.into_iter()
        .filter_map(|n| serde_json::from_value(n).ok())
        .collect()
}

pub async fn unread_count() -> usize {
    list().await.into_iter().filter(|n| !n.read).count()
}

pub async fn mark_all_read() -> Result<(), RuntimeError> {
    let wrap = storage::read_json(NOTIFICATIONS_FILE)
        .await?
        .unwrap_or_else(|| json!({ "notifications": [] }));
    let mut notes = wrap
        .get("notifications")
        .and_then(|a| a.as_array().cloned())
        .unwrap_or_default();
    for n in &mut notes {
        if let Some(obj) = n.as_object_mut() {
            obj.insert("read".into(), Value::Bool(true));
        }
    }
    storage::write_json(NOTIFICATIONS_FILE, &json!({ "notifications": notes })).await
}

pub async fn delete(id: &str) -> Result<(), RuntimeError> {
    let wrap = storage::read_json(NOTIFICATIONS_FILE)
        .await?
        .unwrap_or_else(|| json!({ "notifications": [] }));
    let notes = wrap
        .get("notifications")
        .and_then(|a| a.as_array().cloned())
        .unwrap_or_default();
    let filtered: Vec<_> = notes
        .into_iter()
        .filter(|n| n.get("id").and_then(|v| v.as_str()) != Some(id))
        .collect();
    storage::write_json(NOTIFICATIONS_FILE, &json!({ "notifications": filtered })).await
}
