//! Chat workflow API for the shared engine.
//!
//! `dms` (the v2 sealed-sender DM workflow + data model) and `outbox` (the
//! transient-failure retry queue) are relocated verbatim from hey-social —
//! they depend only on the engine modules (crypto/events/identity/runtime/
//! session) plus `profile`. `profile` here is a THIN identity shim, not
//! hey-social's social profile (followers/avatar/bio live only in the
//! social app); the chat workflow only needs the local {did_key, name}.

pub mod dms;
pub mod outbox;
pub mod profile;
