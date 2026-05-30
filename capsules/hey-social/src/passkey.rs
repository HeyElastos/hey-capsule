// Passkey sign-in — sourced from the shared `hey-core` engine instead of a
// local copy.
//
// hey-social's passkey.rs was byte-identical to the engine's. It was blocked
// from sharing only because it returns `RuntimeError`; now that
// crate::runtime::RuntimeError IS the engine's type (see runtime.rs), the
// engine's passkey is type-compatible and re-exporting is a zero-behavior
// dedup. The engine version uses hey_core::runtime::{upstream_fetch} under the
// CapsuleCtx wired in main.rs, so its HTTP calls still hit /api/apps/hey-social/*
// and the cross-capsule shared-identity dual-write is unchanged.
//
// All existing `crate::passkey::*` call sites keep compiling unchanged.
pub use hey_core::passkey::*;
