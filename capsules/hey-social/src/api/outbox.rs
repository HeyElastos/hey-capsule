// Outbox + retry queue for DM publishes — sourced from the shared `hey-core`
// engine instead of a local copy.
//
// This module was byte-identical to the engine's and is self-contained: it
// depends only on `crate::runtime::{peer, storage}` (both now re-exported from
// the engine) and writes to `Hey/dm/outbox.json` via the per-capsule storage
// namespace ("Hey" via CapsuleCtx) — so re-exporting is a zero-behavior-change
// dedup. Same storage path, same retry/backoff, same wire.
//
// All existing `crate::api::outbox::*` call sites keep compiling unchanged.
pub use hey_core::api::outbox::*;
