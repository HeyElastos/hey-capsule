// Federation identity primitives.
//
// Hey's existing 32-byte recovery key (`authKey`) IS the Ed25519 seed. We
// don't introduce a separate signing key — the user's recovery key already
// has the right entropy and lifecycle (saved once at signup, used to prove
// account ownership). Reinterpreting it as an Ed25519 private key gives us
// the federation signing capability without forcing a re-signup.
//
// Nothing in Hey *yet* calls these helpers. They're the substrate for
// Phase 3 federation (signing gossip events, encoding peer identities as
// did:key strings, verifying signatures on inbound gossip).
//
// We use Node's built-in `crypto` (Ed25519 supported since v12) rather than
// pulling in @noble/ed25519. Zero new deps, same primitive.

const crypto = require("crypto");

// Multicodec varint prefix for ed25519-pub (0xed). Two bytes in varint form.
const ED25519_PUB_MULTICODEC = Buffer.from([0xed, 0x01]);

// PKCS8 DER prefix for a 32-byte Ed25519 raw private key. This is the
// fixed ASN.1 header that wraps the raw seed so Node's createPrivateKey
// can parse it. Pre-computed because it never changes.
//
//   30 2e 02 01 00 30 05 06 03 2b 65 70 04 22 04 20
//   │  │  │  │  │  │  │  │  │  │  │  │  │  │  │  │
//   │  │  │  │  │  │  │  │  │  │  │  │  │  │  └──── 32 = length of seed
//   │  │  │  │  │  │  │  │  │  │  │  │  │  └─────── 04 = OCTET STRING
//   │  │  │  │  │  │  │  │  │  │  │  │  └────────── 22 = inner length
//   │  │  │  │  │  │  │  │  │  │  │  └───────────── 04 = OCTET STRING
//   │  │  │  │  │  │  │  └──┴──┴──┴────────────── OID 1.3.101.112 (Ed25519)
//   │  │  │  │  │  │  └─────────────────────────── 03 = OBJECT IDENTIFIER
//   │  │  │  │  │  └────────────────────────────── 06 = OID tag length
//   │  │  │  │  └───────────────────────────────── 05 = SEQUENCE length
//   │  │  │  └──────────────────────────────────── 30 = SEQUENCE (AlgorithmIdentifier)
//   │  │  └─────────────────────────────────────── 00 = version
//   │  └────────────────────────────────────────── 01 = INTEGER length
//   └───────────────────────────────────────────── 02 = INTEGER (version)
const PKCS8_ED25519_PREFIX = Buffer.from([
  0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06,
  0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20,
]);

// SPKI DER prefix for a 32-byte Ed25519 raw public key. Same idea — fixed
// 12-byte header wrapping the raw key bytes so createPublicKey can parse it.
const SPKI_ED25519_PREFIX = Buffer.from([
  0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65,
  0x70, 0x03, 0x21, 0x00,
]);

const BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

const base58Encode = (buf) => {
  if (buf.length === 0) return "";
  let n = 0n;
  for (const b of buf) n = (n << 8n) | BigInt(b);
  let out = "";
  while (n > 0n) {
    out = BASE58_ALPHABET[Number(n % 58n)] + out;
    n /= 58n;
  }
  // Preserve leading zero bytes as leading '1's.
  for (const b of buf) {
    if (b !== 0) break;
    out = "1" + out;
  }
  return out;
};

const base58Decode = (str) => {
  if (str.length === 0) return Buffer.alloc(0);
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
  return Buffer.from(bytes);
};

// Convert Hey's 32-byte hex `authKey` into a Node KeyObject pair. Same input
// always produces the same keypair — that's the whole point: the recovery
// key IS the identity.
const keypairFromAuthKey = (authKeyHex) => {
  if (typeof authKeyHex !== "string" || !/^[0-9a-f]{64}$/i.test(authKeyHex)) {
    throw new Error("authKey must be a 64-char hex string (32 bytes)");
  }
  const seed = Buffer.from(authKeyHex, "hex");
  const pkcs8 = Buffer.concat([PKCS8_ED25519_PREFIX, seed]);
  const privateKey = crypto.createPrivateKey({ key: pkcs8, format: "der", type: "pkcs8" });
  const publicKey = crypto.createPublicKey(privateKey);
  return { privateKey, publicKey };
};

// Export the raw 32-byte public key (suitable for did:key encoding or for
// stuffing into a gossip message header).
const publicKeyToRawBytes = (publicKey) => {
  const der = publicKey.export({ format: "der", type: "spki" });
  // SPKI is fixed 12-byte prefix + 32 raw bytes.
  if (der.length !== 44) {
    throw new Error(`Unexpected SPKI length: ${der.length}`);
  }
  return der.slice(12);
};

// did:key encoding (W3C CCG) — multibase('z' = base58btc) of multicodec
// ed25519-pub + raw pubkey.
const publicKeyToDidKey = (publicKey) => {
  const raw = publicKeyToRawBytes(publicKey);
  const bytes = Buffer.concat([ED25519_PUB_MULTICODEC, raw]);
  return `did:key:z${base58Encode(bytes)}`;
};

// Inverse: parse a did:key string back into a Node public KeyObject. Used
// to verify signatures from peers we've never met.
const didKeyToPublicKey = (didKey) => {
  if (typeof didKey !== "string" || !didKey.startsWith("did:key:z")) {
    throw new Error("Not a did:key:z... string");
  }
  const decoded = base58Decode(didKey.slice("did:key:z".length));
  if (decoded.length !== 34 || decoded[0] !== 0xed || decoded[1] !== 0x01) {
    throw new Error("Not an Ed25519 did:key");
  }
  const raw = decoded.slice(2);
  const spki = Buffer.concat([SPKI_ED25519_PREFIX, raw]);
  return crypto.createPublicKey({ key: spki, format: "der", type: "spki" });
};

// Sign arbitrary bytes (or a string — utf8). Returns hex signature (64 bytes).
const sign = (message, privateKey) => {
  const buf = Buffer.isBuffer(message) ? message : Buffer.from(message, "utf8");
  // Ed25519: algorithm arg must be null per Node docs.
  return crypto.sign(null, buf, privateKey).toString("hex");
};

const verify = (message, signatureHex, publicKey) => {
  const buf = Buffer.isBuffer(message) ? message : Buffer.from(message, "utf8");
  const sig = Buffer.from(signatureHex, "hex");
  if (sig.length !== 64) return false;
  return crypto.verify(null, buf, publicKey, sig);
};

module.exports = {
  keypairFromAuthKey,
  publicKeyToRawBytes,
  publicKeyToDidKey,
  didKeyToPublicKey,
  sign,
  verify,
};
