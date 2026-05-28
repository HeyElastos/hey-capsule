// Runtime HTTP client — Rust port of capsules/hey-social/client/src/lib/runtime.js.
//
// Wraps the Elastos Runtime gateway surface so the rest of the app never
// touches fetch/URLs directly:
//   /api/localhost/*path             — CRUD storage (JSON files on the host)
//   /api/provider/:scheme/:op        — provider bus (ipfs, did, peer, elacity, ...)
//   /api/capability/request|release  — capability tokens (sent as X-Capability-Token)
//
// To be filled in as each surface is ported. Keep the public API in
// sync with the JS module so callers can be ported one at a time.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityToken(pub String);

pub mod storage {
    // localhost:// CRUD — port from runtime.js `storage` namespace.
}

pub mod peer {
    // elastos://peer/hey-v0/* — Carrier gossip via provider bus.
}

pub mod ipfs {
    // elastos://ipfs/* — Kubo-backed media storage.
}

pub mod did {
    // elastos://did/* — DID resolution.
}

pub mod elacity {
    // elastos://elacity/* — Elacity Player capsule for DASH/CENC playback.
    // See reference_elacity_player memory for integration shape.
}
