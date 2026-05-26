// IDB-backed Ed25519 keystore — Hey Social's hardened signing key home.
//
// What changed and why:
//   Before this file existed, session.js kept the 32-byte recovery key
//   ("authKey") in localStorage. Any XSS in Hey Social could read it
//   and impersonate the user forever, on any device that imported the
//   key. The threat model accepted this as an acknowledged gap — see
//   the historical comment at the top of session.js.
//
//   This file closes the gap. We import the Ed25519 seed as a NON-
//   EXTRACTABLE CryptoKey (Web Crypto API), persist the CryptoKey
//   handle in IndexedDB, and zero the raw seed bytes immediately
//   after. From that moment on, signing happens via crypto.subtle.sign
//   — the private key never appears in JS memory or in storage. An
//   XSS attacker can still call sign() on this origin while the tab
//   is open, but cannot exfiltrate the key for use elsewhere or after
//   the session ends.
//
//   IndexedDB structured-clones CryptoKey objects natively — no JWK
//   round-trip required. The non-extractable flag survives the clone.
//
// Browser support: Web Crypto Ed25519 (Chrome 122+, Safari 17+, Firefox
// 130+ — all 2024 releases). If the browser doesn't support it, we
// throw on save() and the caller (session.js) falls back to its legacy
// localStorage path with a console warning.

const DB_NAME = "hey-keystore";
const DB_VERSION = 1;
const STORE = "keys";
const KEY_ID = "signing-v1";

const PKCS8_ED25519_PREFIX = new Uint8Array([
  0x30, 0x2e,                          // SEQUENCE (46 bytes)
  0x02, 0x01, 0x00,                    // INTEGER 0 (version)
  0x30, 0x05,                          // SEQUENCE (5)
  0x06, 0x03, 0x2b, 0x65, 0x70,        // OID 1.3.101.112 (Ed25519)
  0x04, 0x22,                          // OCTET STRING (34)
  0x04, 0x20,                          // inner OCTET STRING (32 bytes)
]);

// SPKI envelope for verify keys — fixed prefix + the 32-byte pubkey.
const SPKI_ED25519_PREFIX = new Uint8Array([
  0x30, 0x2a, 0x30, 0x05, 0x06, 0x03,
  0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
]);

const idbOpen = () =>
  new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, DB_VERSION);
    req.onerror = () => reject(req.error);
    req.onsuccess = () => resolve(req.result);
    req.onupgradeneeded = () => {
      const db = req.result;
      if (!db.objectStoreNames.contains(STORE)) db.createObjectStore(STORE);
    };
  });

const idbDo = async (mode, fn) => {
  const db = await idbOpen();
  try {
    return await new Promise((resolve, reject) => {
      const tx = db.transaction(STORE, mode);
      tx.onerror = () => reject(tx.error);
      const store = tx.objectStore(STORE);
      const result = fn(store);
      tx.oncomplete = () => resolve(result.value);
      // For requests inside the txn we resolve via their own onsuccess.
      if (result.request) {
        result.request.onsuccess = () => { result.value = result.request.result; };
      }
    });
  } finally {
    db.close();
  }
};

// Build a PKCS#8 envelope wrapping the raw 32-byte seed.
const seedToPkcs8 = (seed) => {
  const out = new Uint8Array(PKCS8_ED25519_PREFIX.length + 32);
  out.set(PKCS8_ED25519_PREFIX, 0);
  out.set(seed, PKCS8_ED25519_PREFIX.length);
  return out;
};

// Import a 32-byte seed as a non-extractable Ed25519 sign key. The seed
// is zeroed inside this function before return, so callers don't need
// to scrub it themselves (but they SHOULD discard their copy too).
const importSeedAsNonExtractable = async (seed) => {
  if (!(seed instanceof Uint8Array) || seed.length !== 32) {
    throw new Error("seed must be 32 bytes");
  }
  if (!crypto.subtle || typeof crypto.subtle.importKey !== "function") {
    throw new Error("Web Crypto unavailable");
  }
  const pkcs8 = seedToPkcs8(seed);
  let privKey;
  try {
    privKey = await crypto.subtle.importKey(
      "pkcs8", pkcs8, { name: "Ed25519" }, /* extractable */ false, ["sign"]
    );
  } catch (err) {
    // Wipe before bubbling
    seed.fill(0); pkcs8.fill(0);
    throw new Error(
      "Browser doesn't expose Ed25519 in Web Crypto yet — update to " +
      "Chrome 122+, Safari 17+, or Firefox 130+ for hardened signing."
    );
  }
  // Best-effort wipe of the seed copy we built.
  seed.fill(0);
  pkcs8.fill(0);
  return privKey;
};

// Public API ─────────────────────────────────────────────────────────

// Save a 32-byte Ed25519 seed as a non-extractable signing key in IDB.
// Caller passes the seed bytes (Uint8Array). On success the seed is
// zeroed in place — caller should also drop any other references.
// Returns the CryptoKey ref so the caller can sign immediately.
export const saveSeedAsSigningKey = async (seed) => {
  const privKey = await importSeedAsNonExtractable(seed);
  await idbDo("readwrite", (s) => ({
    request: s.put(privKey, KEY_ID),
  }));
  return privKey;
};

// Load the persisted signing key. Returns the CryptoKey or null if no
// key has been saved yet. Survives page reloads — IDB structure-clones
// the CryptoKey including its non-extractable flag.
export const loadSigningKey = async () => {
  return idbDo("readonly", (s) => ({ request: s.get(KEY_ID) }))
    .then((val) => val || null);
};

// Delete the signing key (on sign-out).
export const deleteSigningKey = async () => {
  await idbDo("readwrite", (s) => ({ request: s.delete(KEY_ID) }));
};

// Sign a message with the loaded signing key. Returns the hex signature.
export const signWithKey = async (message, privKey) => {
  const data =
    typeof message === "string" ? new TextEncoder().encode(message) : message;
  const sig = await crypto.subtle.sign({ name: "Ed25519" }, privKey, data);
  const bytes = new Uint8Array(sig);
  let hex = "";
  for (let i = 0; i < bytes.length; i++) {
    hex += bytes[i].toString(16).padStart(2, "0");
  }
  return hex;
};

// Verify helper for inbound events. Re-imports the pubKey each call.
// The pubKey isn't secret, so extractable is fine.
export const verifyWithPubKey = async (message, signatureHex, publicKey) => {
  try {
    if (!(publicKey instanceof Uint8Array) || publicKey.length !== 32) return false;
    if (typeof signatureHex !== "string" || signatureHex.length !== 128) return false;
    const sig = new Uint8Array(64);
    for (let i = 0; i < 64; i++) {
      sig[i] = parseInt(signatureHex.slice(i * 2, i * 2 + 2), 16);
    }
    const spki = new Uint8Array(SPKI_ED25519_PREFIX.length + 32);
    spki.set(SPKI_ED25519_PREFIX, 0);
    spki.set(publicKey, SPKI_ED25519_PREFIX.length);
    const key = await crypto.subtle.importKey(
      "spki", spki, { name: "Ed25519" }, false, ["verify"]
    );
    const data =
      typeof message === "string" ? new TextEncoder().encode(message) : message;
    return crypto.subtle.verify({ name: "Ed25519" }, key, sig, data);
  } catch {
    return false;
  }
};

// Probe for browser support — used by main.jsx to decide whether to
// attempt the migration or fall through to the legacy path with a
// console warning.
export const ed25519Supported = async () => {
  try {
    const seed = new Uint8Array(32);
    seed[0] = 1; // not all zeros (some implementations refuse)
    await importSeedAsNonExtractable(seed);
    return true;
  } catch {
    return false;
  }
};
