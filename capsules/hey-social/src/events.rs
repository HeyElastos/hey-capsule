// Signed federation events — now sourced from the shared `hey-core` engine.
//
// This module was hey-social's own copy and had pulled AHEAD of the engine: it
// carried the provider-backed (no-tap) signing path in `create_signed_event`.
// That path has been promoted INTO the engine (hey_core::events), so the engine
// is now a superset and we re-export it — one canonicalization + signing core
// shared by hey-social and hey-chat (signature interop is load-bearing, so a
// single byte-identical implementation is the point). Same on-wire shape
// (type, payload, sender_did, ts, signature), same sorted-keys canonicalize.
pub use hey_core::events::*;
