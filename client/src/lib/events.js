// Browser-side port of server/utils/events.js.
//
// Same on-wire shape (type, payload, sender_did, ts, signature) and the same
// sorted-keys canonicalization so signatures survive JSON wire round-trips.
//
// Used for every federated event Hey publishes: chat messages, post events,
// profile updates, presence announcements. Carrier carries the JSON; this
// module produces it and verifies inbound copies.
//
// HARDENED (2026-05-26): the sign path is now async because the keypair
// holds a non-extractable Web Crypto CryptoKey (see lib/keystore.js).
// The raw Ed25519 seed is never in JS memory after sign-in.
// Verify stays sync (noble), since it only needs the public key bytes
// which are not secret.

import { publicKeyToDidKey, didKeyToPublicKey, sign as nobleSign, verify } from "./identity";
import { signWithKey } from "./keystore";

// Sort-keys JSON. Required so sign(here) and verify(elsewhere) agree on the
// exact byte sequence even if intermediate hops re-encode the JSON.
export const canonicalize = (value) => {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) {
    return "[" + value.map(canonicalize).join(",") + "]";
  }
  const keys = Object.keys(value).sort();
  return (
    "{" +
    keys.map((k) => JSON.stringify(k) + ":" + canonicalize(value[k])).join(",") +
    "}"
  );
};

const bytesToSign = ({ type, payload, sender_did, ts }) =>
  canonicalize({ type, payload, sender_did, ts });

// Construct a signed envelope. `keypair` comes from session.getKeypair():
//   { didKey, publicKey, privKey: CryptoKey }   (hardened path)
//   { didKey, publicKey, seed: Uint8Array, _legacy: true }   (legacy fallback)
//
// async because the hardened path calls crypto.subtle.sign(). Legacy
// stays in-process via noble for browsers without Web Crypto Ed25519.
export const createSignedEvent = async ({ type, payload }, keypair) => {
  if (typeof type !== "string" || !type) {
    throw new Error("event.type is required");
  }
  if (payload === undefined) {
    throw new Error("event.payload is required");
  }
  if (!keypair || !keypair.publicKey) {
    throw new Error("keypair missing — sign in first");
  }
  const sender_did = publicKeyToDidKey(keypair.publicKey);
  const ts = Date.now();
  const message = bytesToSign({ type, payload, sender_did, ts });

  let signature;
  if (keypair.privKey) {
    signature = await signWithKey(message, keypair.privKey);
  } else if (keypair.seed) {
    // Legacy fallback for browsers without Web Crypto Ed25519. The
    // session.js warning already told the user to update.
    signature = nobleSign(message, keypair.seed);
  } else {
    throw new Error("keypair has neither privKey nor seed — cannot sign");
  }
  return { type, payload, sender_did, ts, signature };
};

// Verify a received event. Never throws on malformed input (DoS-safe).
// Stays sync because verify is cheap and pubKey-only.
export const verifySignedEvent = (event) => {
  if (!event || typeof event !== "object") {
    return { valid: false, reason: "not-an-object" };
  }
  const { type, payload, sender_did, ts, signature } = event;
  if (typeof type !== "string" || !type) return { valid: false, reason: "bad-type" };
  if (payload === undefined) return { valid: false, reason: "no-payload" };
  if (typeof sender_did !== "string" || !sender_did.startsWith("did:key:z")) {
    return { valid: false, reason: "bad-sender_did" };
  }
  if (!Number.isInteger(ts) || ts <= 0) return { valid: false, reason: "bad-ts" };
  if (typeof signature !== "string" || signature.length !== 128) {
    return { valid: false, reason: "bad-signature-shape" };
  }

  let pubKey;
  try {
    pubKey = didKeyToPublicKey(sender_did);
  } catch {
    return { valid: false, reason: "unresolvable-did" };
  }

  const ok = verify(bytesToSign({ type, payload, sender_did, ts }), signature, pubKey);
  return ok ? { valid: true } : { valid: false, reason: "signature-mismatch" };
};
