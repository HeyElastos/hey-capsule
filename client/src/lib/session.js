// Session — persists the user's signing identity across page reloads.
//
// HARDENED (2026-05-26): the Ed25519 private key is now imported as a
// NON-EXTRACTABLE Web Crypto CryptoKey and held in IndexedDB. The raw
// recovery seed never lives in JS memory or storage after import.
//
// Public surface is unchanged for sync callers:
//   getKeypair()    → { didKey, publicKey: Uint8Array, privKey: CryptoKey } | null
//   getDidKey()     → string | null
//
// Setters changed to async:
//   setSession(authKey)   → Promise<void>    imports + persists
//   clearSession()        → Promise<void>    wipes IDB + cache
//   initSession()         → Promise<void>    boot-time load + legacy migration
//
// The sync getters work because initSession() populates a module-level
// cache before the React tree mounts (main.jsx awaits it). Without the
// init step the getters return null and callers will fail their
// "Not signed in" guard cleanly.
//
// Threat-model improvement:
//   Before: XSS in Hey could read localStorage.authKey → use the
//           recovery seed forever, on any device, in any process.
//   After:  XSS can call crypto.subtle.sign(privKey, msg) on this
//           origin while a tab is open, but cannot exfiltrate the key
//           for use after the user closes the tab or on another origin.
//
// Fallback: if Web Crypto Ed25519 isn't available (very old browser),
// we keep the legacy localStorage path with a console.warn — better
// than refusing to load.

import { expandKeypair, x25519FromSeed, mlKemFromSeed } from "./identity";
import {
  saveSeedAsSigningKey,
  loadSigningKey,
  deleteSigningKey,
  ed25519Supported as cryptoEd25519Supported,
} from "./keystore";
import * as heyVault from "./vault";

// Module-level sync cache, populated by initSession() at boot or
// setSession() during signup/sign-in. Read by getDidKey() / getKeypair().
let cached = null;

// Legacy localStorage key — read once at migration time, then deleted.
// New writes never touch localStorage when Web Crypto Ed25519 works.
const LEGACY_AUTHKEY_LS = "hey-capsule-session";

// Public-key cache lives in localStorage (the public key isn't secret).
// It lets us know the DID across reloads without touching IDB on every
// getDidKey() call.
const PUBKEY_LS = "hey-public-identity";

// X25519 + ML-KEM-768 keypairs for hybrid PQ E2E. Stored as plain bytes
// because Web Crypto exposes neither ML-KEM nor non-extractable X25519
// yet. XSS on this origin could exfiltrate them — documented in the
// architecture notes. Ed25519 stays hardened in IDB.
const PQ_LS = "hey-pq-identity";

const hexToBytes = (hex) => {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
};

const readPubIdentity = () => {
  try {
    const raw = localStorage.getItem(PUBKEY_LS);
    return raw ? JSON.parse(raw) : null;
  } catch { return null; }
};

const writePubIdentity = (data) => {
  try { localStorage.setItem(PUBKEY_LS, JSON.stringify(data)); }
  catch { /* private-mode storage refusal */ }
};

const clearPubIdentity = () => {
  try { localStorage.removeItem(PUBKEY_LS); } catch { /* ignore */ }
};

// ── Sign-up / sign-in: persist the seed as a non-extractable CryptoKey ──

// Derive + persist the PQ + X25519 keypairs from the same seed.
// IMPORTANT: must run BEFORE keystore zeroes the seed (saveSeedAsSigningKey
// scrubs it in place). Caller passes the seed bytes; we return the two
// extra keypairs to store in `cached`.
const setupPQKeys = (seed) => {
  const x = x25519FromSeed(seed);
  const k = mlKemFromSeed(seed);
  try {
    localStorage.setItem(
      PQ_LS,
      JSON.stringify({
        x25519: { pub: bytesToHex(x.publicKey), priv: bytesToHex(x.privateKey) },
        kem:    { pub: bytesToHex(k.publicKey), priv: bytesToHex(k.secretKey) },
      }),
    );
  } catch {}
  return { x25519: x, kem: k };
};

const loadPQKeys = () => {
  try {
    const raw = localStorage.getItem(PQ_LS);
    if (!raw) return null;
    const j = JSON.parse(raw);
    return {
      x25519: {
        publicKey: hexToBytes(j.x25519.pub),
        privateKey: hexToBytes(j.x25519.priv),
      },
      kem: {
        publicKey: hexToBytes(j.kem.pub),
        secretKey: hexToBytes(j.kem.priv),
      },
    };
  } catch { return null; }
};

const clearPQKeys = () => {
  try { localStorage.removeItem(PQ_LS); } catch {}
};

export const setSession = async (authKey) => {
  if (!authKey) return clearSession();
  // Derive the keypair WITHOUT relying on the keystore (we want the
  // public key + didKey regardless of whether the hardened path works).
  const { seed, publicKey, didKey } = expandKeypair(authKey);

  // Set up PQ + X25519 BEFORE the keystore zeroes `seed` in place.
  const pq = setupPQKeys(seed);

  // Try the hardened path first.
  let privKey = null;
  if (await cryptoEd25519Supported()) {
    try {
      privKey = await saveSeedAsSigningKey(seed);
      // saveSeedAsSigningKey zeroes the seed in place. Done.
    } catch (err) {
      console.warn("[hey] non-extractable key save failed; falling back", err);
    }
  }

  const pubBundle = {
    didKey,
    pubKeyHex: bytesToHex(publicKey),
    x25519Pub: bytesToHex(pq.x25519.publicKey),
    kemPub: bytesToHex(pq.kem.publicKey),
  };

  if (privKey) {
    // Hardened path: cache the handle + pubkey/didKey, drop the seed.
    cached = { didKey, publicKey, privKey, ...pq };
    writePubIdentity(pubBundle);
    // Make sure no legacy seed lingers in localStorage.
    try { localStorage.removeItem(LEGACY_AUTHKEY_LS); } catch {}
  } else {
    // Legacy fallback: seed-in-localStorage. Keep noble-compatible
    // shape so legacy events.js path keeps working if anyone hits it.
    cached = { didKey, publicKey, seed, privKey: null, _legacy: true, ...pq };
    writePubIdentity(pubBundle);
    try {
      localStorage.setItem(LEGACY_AUTHKEY_LS, JSON.stringify({ authKey }));
    } catch { /* private mode */ }
    console.warn(
      "[hey] hardened key store unavailable — using legacy localStorage seed. " +
      "XSS in Hey could exfiltrate the signing key. Update your browser."
    );
  }
};

// ── Boot: load identity from IDB or migrate legacy localStorage seed ──

export const initSession = async () => {
  // Already initialized? Skip.
  if (cached) return;

  // Path 1: hardened key already in IDB.
  const privKey = await loadSigningKey().catch(() => null);
  const pubData = readPubIdentity();
  const pq = loadPQKeys();
  if (privKey && pubData && pubData.didKey && pubData.pubKeyHex && pq) {
    cached = {
      didKey: pubData.didKey,
      publicKey: hexToBytes(pubData.pubKeyHex),
      privKey,
      x25519: pq.x25519,
      kem: pq.kem,
    };
    // Ensure no legacy seed remains.
    try { localStorage.removeItem(LEGACY_AUTHKEY_LS); } catch {}
    return;
  }

  // Path 2: legacy localStorage seed — migrate it to IDB if Web Crypto
  // is available, then wipe localStorage.
  let legacy = null;
  try {
    const raw = localStorage.getItem(LEGACY_AUTHKEY_LS);
    legacy = raw ? JSON.parse(raw) : null;
  } catch { legacy = null; }
  if (legacy?.authKey) {
    try {
      await setSession(legacy.authKey);
      // setSession either persisted via IDB and cleared localStorage,
      // or kept the legacy path with a warning. Either way we're done.
      return;
    } catch (err) {
      console.warn("[hey] legacy seed migration failed", err);
    }
  }

  // Path 3: nothing — user isn't signed in yet.
  cached = null;
};

// ── Sync getters (still cheap, used everywhere) ──────────────────────

export const getKeypair = () => cached || null;

export const getDidKey = () => cached?.didKey || null;

// ── Sign-out ─────────────────────────────────────────────────────────

export const clearSession = async () => {
  cached = null;
  clearPubIdentity();
  clearPQKeys();
  try { localStorage.removeItem(LEGACY_AUTHKEY_LS); } catch { /* ignore */ }
  await deleteSigningKey().catch(() => { /* IDB may be unavailable */ });
  // Also wipe the vault master key from memory on signout. The wraps
  // file on disk is unaffected — a future signin can still unlock it.
  try { heyVault.lockVault(); } catch { /* ignore */ }
};

// ── Internal helper ──────────────────────────────────────────────────

const bytesToHex = (bytes) => {
  let hex = "";
  for (let i = 0; i < bytes.length; i++) hex += bytes[i].toString(16).padStart(2, "0");
  return hex;
};
