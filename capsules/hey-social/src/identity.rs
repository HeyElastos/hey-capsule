// Identity primitives (did:key + Ed25519) — sourced from the shared
// `hey-core` engine instead of a local copy.
//
// This module was byte-identical to the engine's `identity` and is a pure
// leaf (depends only on ed25519-compact + sha2, no runtime/session coupling),
// so re-exporting it is a zero-behavior-change dedup. did:key strings,
// the cross-capsule PRF input, and all signing/verification stay exactly the
// same — they're now just defined in one place.
//
// All existing `crate::identity::*` call sites keep compiling unchanged.
pub use hey_core::identity::*;
