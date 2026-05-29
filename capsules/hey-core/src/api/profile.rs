//! Minimal identity-profile shim for the shared chat engine.
//!
//! hey-social's full `profile.rs` carries social state (followers/following,
//! avatar, bio, follow publish/IPFS). The chat workflow only needs the local
//! identity's `{did_key, name}`, so `hey-core` ships this thin shim instead
//! of the social profile. `ensure_profile()` reads the signed-in `Session` —
//! the `did:key:z…` derived from the passkey PRF is the federated identity;
//! we deliberately never consult the runtime principal (`person:local:…`),
//! which is a different ontology and would mis-display as the user's DID.
//!
//! The fields here are exactly what `api::dms` reads off "my profile"
//! (`.did_key`, `.name`). When hey-social adopts `hey-core`, its richer
//! profile can layer on top; the chat engine never needs more than this.

use crate::runtime::RuntimeError;
use crate::session;

/// The identity fields the chat workflow needs from "my profile".
pub struct IdentityProfile {
    pub did_key: String,
    pub name: String,
}

/// Resolve the local identity from the signed-in session. Errors if there
/// is no session (not signed in).
pub async fn ensure_profile() -> Result<IdentityProfile, RuntimeError> {
    let s = session::current().ok_or_else(|| RuntimeError::new("Not signed in"))?;
    Ok(IdentityProfile {
        did_key: s.did_key,
        name: s.name,
    })
}
