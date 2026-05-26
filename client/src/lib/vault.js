// Hey Social vault — passkey-PRF backed envelope encryption.
//
// Mirrors the architecture of hey-home's hey-vault.js (in the
// elastos-runtime-ynh capsule). The user's hardware authenticator
// holds the secret that derives the vault master key on every unlock;
// the master key lives in browser memory only between unlock and lock.
//
// Each app uses a different PRF input so the same passkey produces
// independent per-app vault keys — Hey Social can't decrypt hey-home's
// sealed data and vice versa, even though both apps run on the same
// origin under the runtime gateway.
//
// Wraps file is stored at the runtime's localhost-provider:
//   /api/localhost/Users/self/.AppData/LocalHost/Hey/vault-wraps.json
// Format:
//   { v: 1, createdAt, wraps: { prf: {iv,wrapped}, recovery: {iv,wrapped,salt} } }
// The wraps are AES-KW envelopes. Without the PRF output (or the user's
// recovery key + the per-wrap salt), the wrapped master key is
// cryptographically inert.
//
// Browser support: WebAuthn PRF extension — Chrome 119+, Edge 119+,
// Safari 18+, Firefox 132+. If the authenticator doesn't speak PRF the
// vault simply isn't initialized; signup/sign-in still work, sensitive
// data just stays at the device-keyed at-rest encryption layer that
// the runtime's localhost-provider already does.

import { storage } from "./runtime";

const VAULT_VERSION = 1;
const WRAPS_PATH = "vault-wraps.json";
const PRF_INPUT_BYTES = new TextEncoder().encode("hey-social-vault-v1");

let masterKey = null; // CryptoKey or null

// ── encoding helpers ────────────────────────────────────────────────

const bytesToHex = (bytes) => {
  let hex = "";
  for (let i = 0; i < bytes.length; i++) hex += bytes[i].toString(16).padStart(2, "0");
  return hex;
};
const hexToBytes = (hex) => {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  return out;
};

// ── WebAuthn PRF helpers ───────────────────────────────────────────

const prfPlausible = () =>
  typeof navigator !== "undefined" &&
  !!navigator.credentials &&
  typeof PublicKeyCredential !== "undefined";

// Build the `extensions.prf` block to attach to any WebAuthn options.
// Callers fold this into their existing create/get options so we don't
// have to mount our own ceremonies.
export const prfExtension = () => ({
  prf: { eval: { first: PRF_INPUT_BYTES.buffer } },
});

// Extract the PRF output from a WebAuthn assertion or attestation.
// Returns Uint8Array(32) or null when the authenticator didn't produce
// one (PRF unsupported or not requested).
export const extractPrfOutput = (credentialOrAssertion) => {
  if (!credentialOrAssertion?.getClientExtensionResults) return null;
  const ext = credentialOrAssertion.getClientExtensionResults();
  const first = ext?.prf?.results?.first;
  return first ? new Uint8Array(first) : null;
};

// ── master-key wrap / unwrap ───────────────────────────────────────

// Derive an AES-GCM wrap key from raw secret bytes via HKDF.
const deriveWrapKey = async (secretBytes, info) => {
  const km = await crypto.subtle.importKey(
    "raw", secretBytes, "HKDF", false, ["deriveKey"]
  );
  return crypto.subtle.deriveKey(
    {
      name: "HKDF",
      hash: "SHA-256",
      salt: new Uint8Array(),
      info: new TextEncoder().encode(info),
    },
    km,
    { name: "AES-GCM", length: 256 },
    false,
    ["encrypt", "decrypt"]
  );
};

const deriveWrapKeyFromRecovery = async (recoveryHex, saltBytes) => {
  const km = await crypto.subtle.importKey(
    "raw", new TextEncoder().encode(recoveryHex), "PBKDF2", false, ["deriveKey"]
  );
  return crypto.subtle.deriveKey(
    { name: "PBKDF2", salt: saltBytes, iterations: 600_000, hash: "SHA-256" },
    km,
    { name: "AES-GCM", length: 256 },
    false,
    ["encrypt", "decrypt"]
  );
};

const wrap = async (mkExtractable, wrapKey) => {
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const wrapped = await crypto.subtle.wrapKey(
    "raw", mkExtractable, wrapKey, { name: "AES-GCM", iv }
  );
  return { iv: bytesToHex(iv), wrapped: bytesToHex(new Uint8Array(wrapped)) };
};

const unwrap = async (wrapObj, wrapKey) => {
  return crypto.subtle.unwrapKey(
    "raw",
    hexToBytes(wrapObj.wrapped),
    wrapKey,
    { name: "AES-GCM", iv: hexToBytes(wrapObj.iv) },
    { name: "AES-GCM", length: 256 },
    false, // non-extractable once unwrapped
    ["encrypt", "decrypt"]
  );
};

// ── Public API ─────────────────────────────────────────────────────

export const isUnlocked = () => masterKey !== null;

export const hasVault = async () => {
  try {
    const wraps = await storage.readJson(WRAPS_PATH);
    return !!(wraps && wraps.v === VAULT_VERSION && wraps.wraps);
  } catch { return false; }
};

// Set up a fresh vault. Generates a random masterKey and wraps it with
// both the PRF output AND the recovery key.
//   prfOutput: Uint8Array(32) — from a fresh enrollment ceremony with PRF
//   recoveryHex: string — user's recovery key (just generated at signup)
// On return, masterKey is in memory + the wraps file is persisted.
export const initVault = async ({ prfOutput, recoveryHex }) => {
  if (!prfOutput || prfOutput.length !== 32) {
    throw new Error("initVault: prfOutput must be 32 bytes");
  }
  if (!/^[0-9a-f]{64}$/i.test(recoveryHex || "")) {
    throw new Error("initVault: recoveryHex must be a 64-char hex string");
  }

  const masterBytes = crypto.getRandomValues(new Uint8Array(32));
  const masterExtractable = await crypto.subtle.importKey(
    "raw", masterBytes, { name: "AES-GCM", length: 256 }, true, ["encrypt", "decrypt"]
  );
  masterKey = await crypto.subtle.importKey(
    "raw", masterBytes, { name: "AES-GCM", length: 256 }, false, ["encrypt", "decrypt"]
  );
  masterBytes.fill(0);

  const prfWrapKey = await deriveWrapKey(prfOutput, "hey-social-prf-v1");
  const prfWrap = await wrap(masterExtractable, prfWrapKey);

  const recoverySalt = crypto.getRandomValues(new Uint8Array(16));
  const recoveryWrapKey = await deriveWrapKeyFromRecovery(recoveryHex, recoverySalt);
  const recoveryWrap = await wrap(masterExtractable, recoveryWrapKey);
  recoveryWrap.salt = bytesToHex(recoverySalt);

  await storage.writeJson(WRAPS_PATH, {
    v: VAULT_VERSION,
    createdAt: new Date().toISOString(),
    wraps: { prf: prfWrap, recovery: recoveryWrap },
  });
};

export const unlockVaultWithPRF = async (prfOutput) => {
  const wraps = await storage.readJson(WRAPS_PATH);
  if (!wraps || wraps.v !== VAULT_VERSION) throw new Error("No vault to unlock");
  if (!wraps.wraps?.prf) throw new Error("No PRF wrap on this vault");
  const wrapKey = await deriveWrapKey(prfOutput, "hey-social-prf-v1");
  masterKey = await unwrap(wraps.wraps.prf, wrapKey);
};

export const unlockVaultWithRecovery = async (recoveryHex) => {
  const wraps = await storage.readJson(WRAPS_PATH);
  if (!wraps || wraps.v !== VAULT_VERSION) throw new Error("No vault to unlock");
  if (!wraps.wraps?.recovery) throw new Error("No recovery wrap on this vault");
  const salt = hexToBytes(wraps.wraps.recovery.salt || "");
  if (salt.length !== 16) throw new Error("Recovery wrap missing salt");
  const wrapKey = await deriveWrapKeyFromRecovery(recoveryHex, salt);
  masterKey = await unwrap(wraps.wraps.recovery, wrapKey);
};

export const lockVault = () => {
  masterKey = null;
};

// ── envelope I/O ───────────────────────────────────────────────────

export const encryptJson = async (value) => {
  if (!masterKey) throw new Error("Vault locked");
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const pt = new TextEncoder().encode(JSON.stringify(value));
  const ct = await crypto.subtle.encrypt({ name: "AES-GCM", iv }, masterKey, pt);
  return {
    v: VAULT_VERSION,
    iv: bytesToHex(iv),
    ct: bytesToHex(new Uint8Array(ct)),
  };
};

export const decryptJson = async (envelope) => {
  if (!masterKey) throw new Error("Vault locked");
  if (!envelope || envelope.v !== VAULT_VERSION) {
    throw new Error("Not a vault envelope");
  }
  const iv = hexToBytes(envelope.iv);
  const ct = hexToBytes(envelope.ct);
  const pt = await crypto.subtle.decrypt({ name: "AES-GCM", iv }, masterKey, ct);
  return JSON.parse(new TextDecoder().decode(pt));
};

// Convenience helpers — read/write a value via the runtime's storage
// API but with the vault layer transparently applied.
export const writeSealed = async (path, value) => {
  const env = await encryptJson(value);
  return storage.writeJson(path, env);
};

export const readSealed = async (path) => {
  const env = await storage.readJson(path);
  if (env == null) return null;
  return decryptJson(env);
};

// Wipe the master key when the tab unloads. Defensive — JS GC reclaims
// when the tab closes anyway, but this is explicit.
if (typeof window !== "undefined") {
  window.addEventListener("beforeunload", () => { masterKey = null; });
}
