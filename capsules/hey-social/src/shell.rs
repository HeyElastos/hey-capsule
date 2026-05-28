// Cross-capsule shared identity — Rust port of capsules/hey-social/client/src/lib/shell.js.
//
// The home shell + every Hey capsule reads/writes one shared profile.json
// under the principal root. Two paths are dual-written for the v0.3 → v0.4
// transition (canonical and legacy) so older readers keep working.
//
//   .AppData/ElastOS/Identity/profile.json    (canonical, doc-aligned)
//   .AppData/Identity/profile.json             (legacy — upstream home shell)

use serde_json::{json, Value};

use crate::runtime::{shared_read_json, shared_write_json, RuntimeError};

pub const CANONICAL_PATH: &str = ".AppData/ElastOS/Identity/profile.json";
pub const LEGACY_PATH: &str = ".AppData/Identity/profile.json";

pub async fn read_shared_identity() -> Result<Option<Value>, RuntimeError> {
    if let Ok(Some(v)) = shared_read_json(CANONICAL_PATH).await {
        return Ok(Some(v));
    }
    shared_read_json(LEGACY_PATH).await
}

pub async fn write_shared_identity(profile: &Value) {
    let _ = shared_write_json(CANONICAL_PATH, profile).await;
    let _ = shared_write_json(LEGACY_PATH, profile).await;
}

// Build a fully-formed profile envelope from the bits the runtime-signin
// flow has: name + didKey + recoveryKeyHash. Other fields default to safe
// values so other capsules don't need to special-case missing keys.
pub fn build_profile(
    name: &str,
    did_key: &str,
    recovery_key_hash: &str,
    created_by: &str,
) -> Value {
    json!({
        "name": name,
        "didKey": did_key,
        "recoveryKeyHash": recovery_key_hash,
        "passkeys": [],
        "avatar": "",
        "bio": "",
        "createdAt": iso_now(),
        "createdBy": created_by,
    })
}

fn iso_now() -> String {
    js_sys::Date::new_0()
        .to_iso_string()
        .as_string()
        .unwrap_or_default()
}
