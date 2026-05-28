// Posts API — Rust port of the post-related slice of
// capsules/hey-social/client/src/api/auth.js.
//
// Storage layout (same as React):
//   Hey/posts/feed.json                  — [{ id, ts, author, post_cid }, ...]
//   Hey/posts/by-id/<id>.json            — full post record (incl. overlays)
//
// What's here today:
//   * list_feed / read_post / write_post / delete_post   — storage CRUD
//   * react_to_post / repost / add_comment               — overlay edits
//
// What's NOT here yet (vs. the React reference):
//   * post.create.v2 dag-cbor IPLD encoding + ipfs.addBytes pin
//     → blocked on porting lib/ipld.js (no Rust dag-cbor crate yet).
//   * Federated peer.publish of overlay events → relies on event signing
//     which IS ported (events.rs). Wired up where it doesn't depend on
//     IPLD; explicitly disabled with a TODO where it does.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::api::profile::ensure_profile;
use crate::events::create_signed_event;
use crate::ipld;
use crate::runtime::{ipfs, peer, storage, RuntimeError};
use crate::session;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaTile {
    #[serde(default)]
    pub url: String,
    pub cid: String,
    #[serde(rename = "type")]
    pub media_type: String, // "photo" | "video"
    #[serde(default)]
    pub mime: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Comment {
    pub id: String,
    #[serde(default)]
    pub user_id: String,
    #[serde(rename = "userDid", default)]
    pub user_did: String,
    #[serde(rename = "userName", default)]
    pub user_name: String,
    #[serde(default)]
    pub text: String,
    #[serde(rename = "parentId", default)]
    pub parent_id: Option<String>,
    #[serde(rename = "createdAt", default)]
    pub created_at: String,
    #[serde(default)]
    pub reactions: serde_json::Map<String, Value>,
    #[serde(default)]
    pub ts: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Post {
    pub id: String,
    #[serde(rename = "userId", default)]
    pub user_id: String,
    #[serde(rename = "userDid")]
    pub user_did: String,
    #[serde(rename = "userName", default)]
    pub user_name: String,
    #[serde(rename = "userAvatar", default)]
    pub user_avatar: String,
    #[serde(default)]
    pub caption: String,
    #[serde(default)]
    pub images: Vec<MediaTile>,
    #[serde(rename = "createdAt", default)]
    pub created_at: String,
    #[serde(default)]
    pub reactions: serde_json::Map<String, Value>,
    #[serde(default)]
    pub reposts: Vec<String>,
    #[serde(default)]
    pub comments: Vec<Comment>,
    pub ts: i64,
    #[serde(rename = "post_cid", default, skip_serializing_if = "Option::is_none")]
    pub post_cid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeedEntry {
    pub id: String,
    pub ts: i64,
    pub author: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_cid: Option<String>,
}

const FEED_INDEX: &str = "posts/feed.json";

fn post_path(id: &str) -> String {
    format!("posts/by-id/{id}.json")
}

pub async fn read_post(id: &str) -> Result<Option<Post>, RuntimeError> {
    match storage::read_json(&post_path(id)).await? {
        Some(v) => Ok(serde_json::from_value(v).ok()),
        None => Ok(None),
    }
}

pub async fn write_post(post: &Post) -> Result<(), RuntimeError> {
    let v = serde_json::to_value(post).map_err(|e| RuntimeError::new(format!("serialize: {e}")))?;
    storage::write_json(&post_path(&post.id), &v).await
}

pub async fn read_feed_index() -> Result<Vec<FeedEntry>, RuntimeError> {
    match storage::read_json(FEED_INDEX).await? {
        Some(v) => Ok(serde_json::from_value::<Vec<FeedEntry>>(v).unwrap_or_default()),
        None => Ok(Vec::new()),
    }
}

pub async fn write_feed_index(idx: &[FeedEntry]) -> Result<(), RuntimeError> {
    let v = serde_json::to_value(idx).map_err(|e| RuntimeError::new(format!("serialize: {e}")))?;
    storage::write_json(FEED_INDEX, &v).await
}

// Pull the most recent N posts from the local feed cache.
pub async fn get_posts(limit: usize) -> Result<Vec<Post>, RuntimeError> {
    let idx = read_feed_index().await?;
    let mut out = Vec::with_capacity(idx.len().min(limit));
    for entry in idx.into_iter().take(limit) {
        if let Some(p) = read_post(&entry.id).await? {
            out.push(p);
        }
    }
    Ok(out)
}

pub async fn get_post(id: &str) -> Result<Option<Post>, RuntimeError> {
    read_post(id).await
}

pub async fn get_user_posts(did_or_id: &str) -> Result<Vec<Post>, RuntimeError> {
    let me = ensure_profile().await.ok();
    let target_did = if did_or_id.starts_with("did:key:z") {
        did_or_id.to_string()
    } else {
        me.as_ref()
            .filter(|m| m.id == did_or_id)
            .map(|m| m.did_key.clone())
            .unwrap_or_else(|| did_or_id.to_string())
    };
    let all = get_posts(500).await?;
    Ok(all.into_iter().filter(|p| p.user_did == target_did).collect())
}

// Sign a federation event and publish it on the given Carrier topic.
// Mirrors the React signEventAndPublish helper.
async fn sign_and_publish(
    topic: &str,
    event_type: &str,
    payload: Value,
) -> Result<(), RuntimeError> {
    let s = session::current().ok_or_else(|| RuntimeError::new("Not signed in"))?;
    let evt = create_signed_event(event_type, payload, &s.auth_key_hex)
        .map_err(|e| RuntimeError::new(format!("sign event: {e}")))?;
    let wire = crate::events::to_wire_string(&evt);
    peer::publish(peer::PublishArgs {
        topic,
        message: &wire,
        sender_id: &evt.sender_did,
        ts: evt.ts,
        signature: &evt.signature,
    })
    .await
    .map(|_| ())
}

fn now_ms() -> i64 {
    js_sys::Date::now() as i64
}

// Upload a file to IPFS via the runtime. Runs the bytes through
// hey-transcoder first (image → WebP @ 2048px, video → H.264 @ 1080p),
// falling through to the original bytes if the transcoder capsule
// isn't installed. Returns the media tile.
pub async fn ipfs_upload_media(
    bytes: &[u8],
    filename: &str,
    mime: &str,
) -> Result<MediaTile, RuntimeError> {
    let processed = crate::runtime::transcoder::process_for_upload(bytes, mime)
        .await
        .unwrap_or_else(|_| crate::runtime::transcoder::Processed {
            bytes: bytes.to_vec(),
            mime: mime.into(),
            transcoded: false,
        });
    let resp = ipfs::add_bytes(&processed.bytes, filename, true).await?;
    let cid = resp
        .get("data")
        .and_then(|d| d.get("cid"))
        .and_then(|c| c.as_str())
        .or_else(|| resp.get("cid").and_then(|c| c.as_str()))
        .map(String::from)
        .ok_or_else(|| RuntimeError::new("IPFS add_bytes returned no CID"))?;
    let media_type = if processed.mime.starts_with("video/") {
        "video"
    } else {
        "photo"
    };
    Ok(MediaTile {
        url: format!("elastos://{cid}"),
        cid,
        media_type: media_type.into(),
        mime: processed.mime,
        name: filename.into(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CreatePostArgs {
    #[serde(default)]
    pub caption: String,
    #[serde(default)]
    pub images: Vec<MediaTile>,
}

// Create a new post: encode the immutable body to dag-cbor → pin to IPFS
// → publish a thin post.create.v2 envelope on the author's Carrier topic
// → cache the full record locally for own-feed rendering and overlay state.
//
// Matches the React createPost in capsules/hey-social/client/src/api/auth.js.
// Falls through to local-only if any of the network steps fail (IPFS down,
// peer provider down, etc.) so the post still appears in the user's feed.
pub async fn create_post(args: CreatePostArgs) -> Result<Post, RuntimeError> {
    let me = ensure_profile().await?;
    let id = uuid::Uuid::new_v4().to_string();
    let ts = now_ms();
    let mut post = Post {
        id: id.clone(),
        user_id: me.id.clone(),
        user_did: me.did_key.clone(),
        user_name: me.name.clone(),
        user_avatar: me.avatar.clone(),
        caption: args.caption.chars().take(2200).collect(),
        images: args.images,
        created_at: js_sys::Date::new_0()
            .to_iso_string()
            .as_string()
            .unwrap_or_default(),
        reactions: serde_json::Map::new(),
        reposts: Vec::new(),
        comments: Vec::new(),
        ts,
        post_cid: None,
    };

    // 1. Encode the immutable body to dag-cbor and pin to IPFS.
    match ipld::encode_post_metadata(&post) {
        Ok(bytes) => {
            let filename = format!("post-{id}.cbor");
            match ipfs::add_bytes(&bytes, &filename, true).await {
                Ok(resp) => {
                    let cid = resp
                        .get("data")
                        .and_then(|d| d.get("cid"))
                        .and_then(|c| c.as_str())
                        .or_else(|| resp.get("cid").and_then(|c| c.as_str()))
                        .map(String::from);
                    if let Some(cid) = cid {
                        post.post_cid = Some(cid.clone());
                        // 2. Publish the post.create.v2 envelope. Receivers
                        //    decode the CID from this and pull the body from
                        //    IPFS, materialize via materialize_post_from_cid.
                        let _ = sign_and_publish(
                            &format!("hey-v0/user/{}/posts", me.did_key),
                            "post.create.v2",
                            json!({ "post_cid": cid }),
                        )
                        .await;
                    }
                }
                Err(e) => {
                    web_sys::console::warn_1(&wasm_bindgen::JsValue::from_str(&format!(
                        "[hey-social] ipfs.add_bytes for post metadata failed: {e}"
                    )));
                }
            }
        }
        Err(e) => {
            web_sys::console::warn_1(&wasm_bindgen::JsValue::from_str(&format!(
                "[hey-social] dag-cbor encode failed (post stays local-only): {e}"
            )));
        }
    }

    // 3. Local cache.
    write_post(&post).await?;
    let mut idx = read_feed_index().await?;
    idx.insert(
        0,
        FeedEntry {
            id: id.clone(),
            ts,
            author: me.did_key.clone(),
            post_cid: post.post_cid.clone(),
        },
    );
    write_feed_index(&idx).await?;
    Ok(post)
}

// Materialize a remote post from a post.create.v2 Carrier event.
// Returns None if the CID can't be fetched or decoded — caller decides
// whether to retry or drop.
pub async fn materialize_post_from_cid(post_cid: &str) -> Option<Post> {
    if post_cid.is_empty() {
        return None;
    }
    let bytes = match ipfs::get_bytes(post_cid, None).await {
        Ok(b) => b,
        Err(_) => return None,
    };
    let body = match ipld::decode_post_metadata(&bytes) {
        Ok(b) => b,
        Err(_) => return None,
    };
    Some(ipld::materialize_from_ipld(body, post_cid.to_string()))
}

pub async fn delete_post(post_id: &str) -> Result<(), RuntimeError> {
    let me = ensure_profile().await?;
    let Some(post) = read_post(post_id).await? else {
        return Err(RuntimeError::new("Post not found"));
    };
    if post.user_did != me.did_key {
        return Err(RuntimeError::new("Not your post"));
    }
    let _ = storage::remove(&post_path(post_id)).await;
    let idx = read_feed_index().await?;
    let filtered: Vec<_> = idx.into_iter().filter(|e| e.id != post_id).collect();
    write_feed_index(&filtered).await?;
    // Best-effort federate the delete to followers (overlay event, no IPLD).
    let _ = sign_and_publish(
        &format!("hey-v0/user/{}/posts", me.did_key),
        "post.delete",
        json!({ "post_id": post_id, "ts": now_ms() }),
    )
    .await;
    Ok(())
}

pub async fn react_to_post(post_id: &str, emoji: &str) -> Result<Post, RuntimeError> {
    let me = ensure_profile().await?;
    let mut post = read_post(post_id)
        .await?
        .ok_or_else(|| RuntimeError::new("Post not found"))?;
    let mut reactions = post.reactions.clone();
    let entry = reactions.entry(emoji.to_string()).or_insert(json!([]));
    let list = entry.as_array_mut().ok_or_else(|| RuntimeError::new("bad reactions shape"))?;
    if let Some(pos) = list.iter().position(|v| v.as_str() == Some(&me.did_key)) {
        list.remove(pos);
    } else {
        list.push(json!(me.did_key));
    }
    if list.is_empty() {
        reactions.remove(emoji);
    }
    post.reactions = reactions;
    write_post(&post).await?;
    let _ = sign_and_publish(
        &format!("hey-v0/user/{}/posts", post.user_did),
        "post.react",
        json!({
            "post_id": post_id,
            "emoji": emoji,
            "reactor_did": me.did_key,
            "ts": now_ms(),
        }),
    )
    .await;
    Ok(post)
}

pub async fn repost(post_id: &str) -> Result<Post, RuntimeError> {
    let me = ensure_profile().await?;
    let mut post = read_post(post_id)
        .await?
        .ok_or_else(|| RuntimeError::new("Post not found"))?;
    let mut set: Vec<String> = post.reposts.clone();
    if !set.contains(&me.did_key) {
        set.push(me.did_key.clone());
    }
    post.reposts = set;
    write_post(&post).await?;
    let _ = sign_and_publish(
        &format!("hey-v0/user/{}/posts", post.user_did),
        "post.repost",
        json!({
            "post_id": post_id,
            "reposter_did": me.did_key,
            "ts": now_ms(),
        }),
    )
    .await;
    Ok(post)
}

pub async fn add_comment(
    post_id: &str,
    text: &str,
    parent_id: Option<String>,
) -> Result<Post, RuntimeError> {
    let me = ensure_profile().await?;
    let mut post = read_post(post_id)
        .await?
        .ok_or_else(|| RuntimeError::new("Post not found"))?;
    let comment = Comment {
        id: uuid::Uuid::new_v4().to_string(),
        user_id: me.id.clone(),
        user_did: me.did_key.clone(),
        user_name: me.name.clone(),
        text: text.chars().take(500).collect(),
        parent_id,
        created_at: js_sys::Date::new_0()
            .to_iso_string()
            .as_string()
            .unwrap_or_default(),
        reactions: serde_json::Map::new(),
        ts: now_ms(),
    };
    post.comments.push(comment.clone());
    write_post(&post).await?;
    let _ = sign_and_publish(
        &format!("hey-v0/user/{}/posts", post.user_did),
        "post.comment",
        json!({
            "post_id": post_id,
            "comment": comment,
        }),
    )
    .await;
    Ok(post)
}

// Convenience: encode a Rust byte slice for upload via the JSON-friendly
// helpers above. Kept here so callers in pages/ don't need to depend on
// base64 directly.
pub fn _bytes_to_b64(b: &[u8]) -> String {
    B64.encode(b)
}

// Currently-signed-in user (unwrap convenience for components).
pub fn current_profile_didkey() -> Option<String> {
    session::current().map(|s| s.did_key)
}
