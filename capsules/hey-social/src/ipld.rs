// IPLD helpers — Rust port of capsules/hey-social/client/src/lib/ipld.js.
//
// Posts have two halves:
//   1. Immutable body (caption, media list, author, ts) — encoded as
//      dag-cbor and pinned to IPFS. The blob's CID is the canonical
//      post identity. Carrier post.create.v2 events only carry the CID.
//   2. Mutable overlays (reactions, comments, reposts) — local-only,
//      not part of the IPLD blob, accumulated via overlay events.
//
// We use ciborium for serialization. dag-cbor canonicalization rules
// we must honor (otherwise @ipld/dag-cbor on the JS side rejects the
// blob on decode):
//
//   * Map keys = strings, sorted by length first then bytewise lex
//   * Definite-length encoding for maps/arrays
//   * Shortest integer encoding (ciborium does this by default)
//   * No NaN/Inf floats (we don't emit any)
//
// Field-declaration order on our serde structs is the canonical map
// key order. Don't reorder fields without re-deriving the canonical
// sort — see comments above each struct.

use ciborium::Value as CborValue;
use serde::{Deserialize, Serialize};

use crate::api::posts::{MediaTile, Post};

pub const IPLD_POST_SCHEMA_VERSION: u32 = 1;

// MediaEntry's keys in dag-cbor canonical order:
//   cid (3), mime (4), name (4), type (4)
//   — for length-4 keys: mime < name < type (lex).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MediaEntry {
    cid: String,
    mime: String,
    name: String,
    #[serde(rename = "type")]
    type_: String,
}

// PostBody's keys in dag-cbor canonical order:
//   v (1), ts (2), media (5), caption (7),
//   author_did (10), created_at (10), author_name (11)
//   — for length-10 keys: author_did < created_at (lex).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PostBody {
    v: u32,
    ts: i64,
    media: Vec<MediaEntry>,
    caption: String,
    author_did: String,
    created_at: String,
    author_name: String,
}

fn build_immutable_body(post: &Post) -> Result<PostBody, String> {
    if post.user_did.is_empty() {
        return Err("post.userDid required".into());
    }
    if post.ts == 0 {
        return Err("post.ts required".into());
    }
    let media = post
        .images
        .iter()
        .map(|m| MediaEntry {
            cid: m.cid.clone(),
            mime: m.mime.clone(),
            name: m.name.clone(),
            type_: if m.media_type == "video" {
                "video".into()
            } else {
                "photo".into()
            },
        })
        .collect();
    Ok(PostBody {
        v: IPLD_POST_SCHEMA_VERSION,
        ts: post.ts,
        media,
        caption: post.caption.clone(),
        author_did: post.user_did.clone(),
        created_at: if post.created_at.is_empty() {
            iso_from_ts(post.ts)
        } else {
            post.created_at.clone()
        },
        author_name: post.user_name.clone(),
    })
}

fn iso_from_ts(ts: i64) -> String {
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ts as f64));
    d.to_iso_string().as_string().unwrap_or_default()
}

pub fn encode_post_metadata(post: &Post) -> Result<Vec<u8>, String> {
    let body = build_immutable_body(post)?;
    let mut buf = Vec::new();
    ciborium::ser::into_writer(&body, &mut buf)
        .map_err(|e| format!("dag-cbor encode: {e}"))?;
    Ok(buf)
}

pub fn decode_post_metadata(bytes: &[u8]) -> Result<DecodedPost, String> {
    let value: CborValue = ciborium::de::from_reader(bytes)
        .map_err(|e| format!("dag-cbor decode: {e}"))?;
    decode_post_value(&value)
}

#[derive(Debug, Clone)]
pub struct DecodedPost {
    pub v: u32,
    pub ts: i64,
    pub caption: String,
    pub author_did: String,
    pub author_name: String,
    pub created_at: String,
    pub media: Vec<MediaTile>,
}

// Tolerant decoder: accepts the canonical dag-cbor body as well as
// slightly-drifted variants (e.g. older posts without created_at).
fn decode_post_value(value: &CborValue) -> Result<DecodedPost, String> {
    let map = value
        .as_map()
        .ok_or_else(|| "post body must be a CBOR map".to_string())?;

    let get = |key: &str| -> Option<&CborValue> {
        map.iter().find_map(|(k, v)| match k {
            CborValue::Text(s) if s == key => Some(v),
            _ => None,
        })
    };

    let v = get("v")
        .and_then(|v| v.as_integer().and_then(|i| i128::from(i).try_into().ok()))
        .unwrap_or(0u32);
    if v != IPLD_POST_SCHEMA_VERSION {
        return Err(format!("unsupported post schema v={v}"));
    }
    let ts = get("ts")
        .and_then(|v| v.as_integer().and_then(|i| i128::from(i).try_into().ok()))
        .unwrap_or(0i64);
    let caption = get("caption").and_then(cbor_str).unwrap_or_default();
    let author_did = get("author_did")
        .and_then(cbor_str)
        .ok_or_else(|| "missing author_did".to_string())?;
    let author_name = get("author_name").and_then(cbor_str).unwrap_or_default();
    let created_at = get("created_at").and_then(cbor_str).unwrap_or_default();

    let media = get("media")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let m = item.as_map()?;
                    let mget = |k: &str| -> Option<String> {
                        m.iter().find_map(|(key, val)| match key {
                            CborValue::Text(s) if s == k => cbor_str(val),
                            _ => None,
                        })
                    };
                    let cid = mget("cid")?;
                    let media_type = mget("type").unwrap_or_else(|| "photo".into());
                    let mime = mget("mime").unwrap_or_default();
                    let name = mget("name").unwrap_or_default();
                    Some(MediaTile {
                        url: format!("elastos://{cid}"),
                        cid,
                        media_type,
                        mime,
                        name,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(DecodedPost {
        v,
        ts,
        caption,
        author_did,
        author_name,
        created_at,
        media,
    })
}

fn cbor_str(v: &CborValue) -> Option<String> {
    match v {
        CborValue::Text(s) => Some(s.clone()),
        _ => None,
    }
}

// Convenience for receivers: given a decoded body + the CID it came
// from, produce a Hey-internal Post record with empty overlays. Caller
// is responsible for generating a local UUID and persisting via
// write_post + write_feed_index.
pub fn materialize_from_ipld(body: DecodedPost, post_cid: String) -> Post {
    let preview_name = if body.author_name.is_empty() {
        format!("{}…", body.author_did.chars().take(14).collect::<String>())
    } else {
        body.author_name.clone()
    };
    Post {
        id: uuid::Uuid::new_v4().to_string(),
        user_id: body.author_did.clone(),
        user_did: body.author_did,
        user_name: preview_name,
        user_avatar: String::new(),
        caption: body.caption,
        images: body.media,
        created_at: body.created_at,
        reactions: serde_json::Map::new(),
        reposts: Vec::new(),
        comments: Vec::new(),
        ts: body.ts,
        post_cid: Some(post_cid),
    }
}
