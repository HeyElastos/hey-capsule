// Hybrid post-quantum E2E encryption for hey-messenger DMs.
//
// Construction:
//   shared_secret = HKDF-SHA256(X25519_dh || ML-KEM-768_secret, info)
//   ciphertext    = ChaCha20-Poly1305(plaintext, key=shared_secret, nonce)
//
// Why hybrid: ML-KEM-768 is the NIST FIPS 203 post-quantum KEM standard
// (formerly Kyber-768). @noble/post-quantum 0.6.1 is SELF-AUDITED only
// — not independently reviewed. Using it as the SOLE encryption layer
// would put us at the mercy of any flaw in either ML-KEM or noble's
// implementation. By combining with classical X25519 we degrade
// gracefully: an attacker must break BOTH primitives to recover the
// plaintext. This is the same hybrid pattern Signal PQXDH and the NIST
// PQ migration guidelines recommend.
//
// This module provides single-shot per-message encryption — there is
// NO key ratchet, so forward secrecy is per-message but not across the
// session. Adding a Signal-PQ-style ratchet is tracked as Phase 6.
//
// Wire format (every field bytes-or-base64 in the JSON envelope):
//   { v: "hpq-1", eph: <32B>, kem: <1088B>, n: <12B>, ct: <varB> }

import { x25519 } from "@noble/curves/ed25519.js";
import { ml_kem768 } from "@noble/post-quantum/ml-kem.js";
import { chacha20poly1305 } from "@noble/ciphers/chacha.js";
import { hkdf } from "@noble/hashes/hkdf.js";
import { sha256 } from "@noble/hashes/sha2.js";

const HKDF_INFO = new TextEncoder().encode("hey-messenger/hpq-1");

const b64encode = (bytes) => {
  let bin = "";
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
  return btoa(bin);
};

const b64decode = (s) => {
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
};

// Deterministic X25519 keypair from an Ed25519 seed. We reuse the same
// seed because @noble/curves derives X25519 directly from 32-byte input.
// The X25519 pubkey is independent of the Ed25519 pubkey (different curve
// math); both can be derived from the same seed without weakening either.
export const x25519FromSeed = (seed) => {
  if (!(seed instanceof Uint8Array) || seed.length !== 32) {
    throw new Error("x25519FromSeed: seed must be 32 bytes");
  }
  const publicKey = x25519.getPublicKey(seed);
  return { privateKey: seed, publicKey };
};

// Generate a fresh ML-KEM-768 keypair. Each user has one; published in
// their profile bundle so peers can encrypt to them.
export const generateMlKemKeypair = () => {
  const seed = new Uint8Array(64);
  crypto.getRandomValues(seed);
  const { publicKey, secretKey } = ml_kem768.keygen(seed);
  seed.fill(0);
  return { publicKey, secretKey };
};

// Derive the symmetric key from the two parallel secrets. Concat order
// matters and must match decryption.
const deriveKey = (x25519Secret, kemSecret) => {
  const ikm = new Uint8Array(x25519Secret.length + kemSecret.length);
  ikm.set(x25519Secret, 0);
  ikm.set(kemSecret, x25519Secret.length);
  const key = hkdf(sha256, ikm, undefined, HKDF_INFO, 32);
  ikm.fill(0);
  return key;
};

// Encrypt a UTF-8 string (or arbitrary bytes) to a recipient identified
// by their X25519 + ML-KEM-768 public keys. The recipient must have
// previously published both pubkeys via their profile bundle.
//
// Returns the envelope as a plain JS object with base64-encoded byte
// fields, ready to drop into a signed event payload.
export const encryptToHybrid = (plaintext, recipientX25519Pub, recipientKemPub) => {
  const data =
    typeof plaintext === "string"
      ? new TextEncoder().encode(plaintext)
      : plaintext instanceof Uint8Array
      ? plaintext
      : new Uint8Array(plaintext);

  // Ephemeral X25519 keypair — fresh per message for partial FS.
  const ephSeed = new Uint8Array(32);
  crypto.getRandomValues(ephSeed);
  const ephPub = x25519.getPublicKey(ephSeed);
  const x25519Secret = x25519.getSharedSecret(ephSeed, recipientX25519Pub);
  ephSeed.fill(0);

  // ML-KEM encapsulation against the recipient's KEM pubkey.
  const { cipherText: kemCt, sharedSecret: kemSecret } =
    ml_kem768.encapsulate(recipientKemPub);

  const key = deriveKey(x25519Secret, kemSecret);
  x25519Secret.fill(0);
  kemSecret.fill(0);

  const nonce = new Uint8Array(12);
  crypto.getRandomValues(nonce);
  const aead = chacha20poly1305(key, nonce);
  const ct = aead.encrypt(data);
  key.fill(0);

  return {
    v: "hpq-1",
    eph: b64encode(ephPub),
    kem: b64encode(kemCt),
    n: b64encode(nonce),
    ct: b64encode(ct),
  };
};

// Decrypt an envelope. Requires the recipient's X25519 + ML-KEM secret
// keys (from session). Throws on tampering / wrong-key / decode error.
export const decryptHybrid = (envelope, myX25519Priv, myKemSecret) => {
  if (!envelope || envelope.v !== "hpq-1") {
    throw new Error("decryptHybrid: unsupported envelope version");
  }
  const ephPub = b64decode(envelope.eph);
  const kemCt = b64decode(envelope.kem);
  const nonce = b64decode(envelope.n);
  const ct = b64decode(envelope.ct);

  const x25519Secret = x25519.getSharedSecret(myX25519Priv, ephPub);
  const kemSecret = ml_kem768.decapsulate(kemCt, myKemSecret);
  const key = deriveKey(x25519Secret, kemSecret);
  x25519Secret.fill(0);
  kemSecret.fill(0);

  const aead = chacha20poly1305(key, nonce);
  const pt = aead.decrypt(ct); // throws on auth failure
  key.fill(0);
  return new TextDecoder().decode(pt);
};

// Tiny self-test, for use in dev consoles + unit checks. Returns true on
// success, throws on any mismatch.
export const __selfTest = () => {
  const a_seed = new Uint8Array(32); crypto.getRandomValues(a_seed);
  const a_x = x25519FromSeed(a_seed);
  const a_k = generateMlKemKeypair();

  const msg = "hello, post-quantum world 🔒";
  const env = encryptToHybrid(msg, a_x.publicKey, a_k.publicKey);
  const out = decryptHybrid(env, a_x.privateKey, a_k.secretKey);
  if (out !== msg) throw new Error(`pqcrypto self-test mismatch: ${out}`);
  return true;
};
