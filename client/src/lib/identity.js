// Browser-side port of server/utils/identity.js.
//
// Same algorithm — 32-byte authKey (hex) reinterpreted as an Ed25519 seed,
// did:key encoding (W3C CCG spec, base58btc + multicodec ed25519-pub prefix).
// Verified against the same RFC 8032 Test 1 vector as the Node version.
//
// Backend uses Node's built-in crypto. Browser uses @noble/curves/ed25519
// — same primitive, same wire format.

import { ed25519 } from "@noble/curves/ed25519.js";

const ED25519_PUB_MULTICODEC = new Uint8Array([0xed, 0x01]);
const BASE58_ALPHABET =
  "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

const hexToBytes = (hex) => {
  if (typeof hex !== "string" || !/^[0-9a-f]+$/i.test(hex) || hex.length % 2) {
    throw new Error("Invalid hex string");
  }
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
};

const bytesToHex = (bytes) =>
  [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");

export const base58Encode = (buf) => {
  if (buf.length === 0) return "";
  let n = 0n;
  for (const b of buf) n = (n << 8n) | BigInt(b);
  let out = "";
  while (n > 0n) {
    out = BASE58_ALPHABET[Number(n % 58n)] + out;
    n /= 58n;
  }
  for (const b of buf) {
    if (b !== 0) break;
    out = "1" + out;
  }
  return out;
};

export const base58Decode = (str) => {
  if (str.length === 0) return new Uint8Array();
  let n = 0n;
  for (const c of str) {
    const idx = BASE58_ALPHABET.indexOf(c);
    if (idx < 0) throw new Error(`Invalid base58 character: ${c}`);
    n = n * 58n + BigInt(idx);
  }
  const bytes = [];
  while (n > 0n) {
    bytes.unshift(Number(n & 0xffn));
    n >>= 8n;
  }
  for (const c of str) {
    if (c !== "1") break;
    bytes.unshift(0);
  }
  return new Uint8Array(bytes);
};

// Convert the user's 32-byte hex authKey into a browser-usable keypair.
// Result is deterministic — same authKey always derives the same public key.
export const keypairFromAuthKey = (authKeyHex) => {
  if (typeof authKeyHex !== "string" || !/^[0-9a-f]{64}$/i.test(authKeyHex)) {
    throw new Error("authKey must be a 64-char hex string (32 bytes)");
  }
  const seed = hexToBytes(authKeyHex);
  const publicKey = ed25519.getPublicKey(seed); // 32 bytes
  return { seed, publicKey };
};

// Generate a fresh 32-byte recovery key. Used at signup.
export const generateAuthKey = () => {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return bytesToHex(bytes);
};

// SHA-256 of the authKey hex — what the server used to store as authKeyHash.
// We replicate it client-side so the same key passes the same check.
export const hashAuthKey = async (authKeyHex) => {
  const buf = new TextEncoder().encode(authKeyHex);
  const digest = await crypto.subtle.digest("SHA-256", buf);
  return bytesToHex(new Uint8Array(digest));
};

// Encode a 32-byte Ed25519 public key as a did:key string.
// Format: did:key: + z (multibase btc) + base58( [0xed 0x01] || pubkey )
export const publicKeyToDidKey = (publicKey) => {
  if (!(publicKey instanceof Uint8Array) || publicKey.length !== 32) {
    throw new Error("publicKey must be 32 bytes");
  }
  const bytes = new Uint8Array(2 + 32);
  bytes.set(ED25519_PUB_MULTICODEC, 0);
  bytes.set(publicKey, 2);
  return `did:key:z${base58Encode(bytes)}`;
};

// Inverse: parse "did:key:z..." back to a 32-byte Ed25519 public key.
// Used to verify signatures from peers we've never met.
export const didKeyToPublicKey = (didKey) => {
  if (typeof didKey !== "string" || !didKey.startsWith("did:key:z")) {
    throw new Error("Not a did:key:z... string");
  }
  const decoded = base58Decode(didKey.slice("did:key:z".length));
  if (decoded.length !== 34 || decoded[0] !== 0xed || decoded[1] !== 0x01) {
    throw new Error("Not an Ed25519 did:key");
  }
  return decoded.slice(2);
};

// Sign arbitrary bytes (or utf-8 string). Returns hex signature (64 bytes).
export const sign = (message, seed) => {
  const data =
    typeof message === "string" ? new TextEncoder().encode(message) : message;
  const sig = ed25519.sign(data, seed);
  return bytesToHex(sig);
};

export const verify = (message, signatureHex, publicKey) => {
  try {
    const data =
      typeof message === "string" ? new TextEncoder().encode(message) : message;
    const sig = hexToBytes(signatureHex);
    if (sig.length !== 64) return false;
    return ed25519.verify(sig, data, publicKey);
  } catch {
    return false;
  }
};

// Convenience wrapper: bundle the keypair + did:key into one object the rest
// of the app can pass around without re-deriving anything.
export const expandKeypair = (authKeyHex) => {
  const { seed, publicKey } = keypairFromAuthKey(authKeyHex);
  return { seed, publicKey, didKey: publicKeyToDidKey(publicKey) };
};
