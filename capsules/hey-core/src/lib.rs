//! Shared chat engine for the Hey capsule pack.
//!
//! Extracted from `hey-social` so `hey-social` and the Rust `hey-messenger`
//! share ONE implementation of the security-critical chat core — the two
//! apps must stay byte-identical to interoperate (one chat network), and a
//! single copy keeps the audit surface single.
//!
//! Built in phases (see memory `hey-messenger-rust-port`):
//!   Phase 1 (this commit): the pure, parameter-free security core —
//!     crypto (PQ-E2E hybrid), identity (did:key + Ed25519), events
//!     (signed federation envelope + canonicalization). Copied verbatim
//!     from hey-social; no `CAPSULE_ID`/namespace coupling.
//!   Later phases: the runtime transport boundary (peer/storage/provider_call/
//!     capability) + session + outbox + passkey + the de-entangled DM
//!     workflow + a chat-only peer receiver + a blobs attachment wrapper.
//!     Those carry per-capsule identity, so they take it via an injected
//!     `CapsuleCtx` rather than baking hey-social's values.

pub mod api;
pub mod crypto;
pub mod ctx;
pub mod events;
pub mod identity;
pub mod passkey;
pub mod peer_receiver;
pub mod runtime;
pub mod session;
