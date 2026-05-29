// Hybrid post-quantum E2E crypto for DMs — now sourced from the shared
// `hey-core` engine instead of a local copy.
//
// This module was previously a verbatim duplicate of the engine's crypto. It
// had drifted to the OLDER `hpq-1` wire format (no padding) while the engine
// moved to `hpq-2` (fixed-size content padding), which meant a hey-social
// client and a hey-messenger client could no longer exchange DMs. Re-exporting
// the engine fixes that incompatibility and collapses two copies of
// security-critical code to one audited implementation.
//
// Safe because the engine is a strict, backward-compatible superset:
//   * key derivation (x25519_from_seed, generate_ml_kem_keypair,
//     keys_from_seed_and_kem, derive_key) is byte-identical → identities and
//     existing key material are unchanged;
//   * decrypt_hybrid still accepts hpq-1 envelopes → no stored/in-flight
//     message becomes unreadable;
//   * we now ENCRYPT to hpq-2 → cross-app interop + SimpleX-style length
//     hardening, plus the Double Ratchet primitives ride along for free.
//
// All existing `crate::crypto::*` call sites keep compiling — the engine
// exposes the same public API (HpqEnvelope, UserKeys, encrypt_to_hybrid,
// decrypt_hybrid, self_test, …) under identical names.
pub use hey_core::crypto::*;
