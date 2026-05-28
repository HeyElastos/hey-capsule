// Shell host detection + shared identity contract.
//
// Hey is a capsule that can be hosted by different desktop shells. We
// treat hey-home as the canonical host; stock `home` works but is a
// degraded fallback. This file owns the cross-shell contract:
//
//   .AppData/SystemServices/Shell/active.json
//     { name, version, writtenAt }   ← whichever shell is "active"
//       writes this on boot. Hey reads it to know its host.
//
//   .AppData/Identity/profile.json
//     { name, didKey, recoveryKeyHash, passkeys, createdAt, createdBy }
//       ← shared identity used by both hey-home and the Hey app, so
//         the user doesn't sign up twice.
//
// Both paths sit OUTSIDE the Hey/ prefix so they land at the per-user
// principal root, where other capsules under the same user can read
// them via the same /api/apps/<their-name>/storage/.AppData/... route
// (or, for the shell, via the privileged /api/localhost/Users/self/*
// route — both hash to the same on-disk location).

import { sharedStorage } from "./runtime";

const SHELL_MARKER_SUFFIX = ".AppData/SystemServices/Shell/active.json";
const SHARED_IDENTITY_SUFFIX = ".AppData/Identity/profile.json";

const safeRead = async (suffix) => {
  try { return await sharedStorage.readJson(suffix); }
  catch { return null; }
};

const safeWrite = async (suffix, value) => {
  try { await sharedStorage.writeJson(suffix, value); return true; }
  catch { return false; }
};

// ── Shell detection ────────────────────────────────────────────────

let cachedShell = null;

export const detectShell = async () => {
  if (cachedShell) return cachedShell;

  // Primary: marker file written by shells that opt in (hey-home does).
  const marker = await safeRead(SHELL_MARKER_SUFFIX);
  if (marker && marker.name) {
    cachedShell = {
      name: marker.name,
      version: marker.version || null,
      hosted: true,
      source: "marker",
    };
    return cachedShell;
  }

  // Fallback: inspect URL + referrer. Stock home doesn't write a marker
  // but its window path will reveal it.
  const haystack = `${document.referrer || ""} ${window.location.href}`;
  if (/\/apps\/hey-home(\/|$)/.test(haystack)) {
    cachedShell = { name: "hey-home", version: null, hosted: true, source: "url" };
  } else if (/\/apps\/home(\/|$)/.test(haystack)) {
    cachedShell = { name: "home", version: null, hosted: true, source: "url" };
  } else {
    cachedShell = { name: "unknown", version: null, hosted: false, source: "none" };
  }
  return cachedShell;
};

export const isHostedByHeyHome = async () => {
  const s = await detectShell();
  return s.name === "hey-home";
};

export const isHostedByStockHome = async () => {
  const s = await detectShell();
  return s.name === "home";
};

// ── Shared identity ────────────────────────────────────────────────

export const readSharedIdentity = () => safeRead(SHARED_IDENTITY_SUFFIX);

export const writeSharedIdentity = (profile) =>
  safeWrite(SHARED_IDENTITY_SUFFIX, profile);

export const deleteSharedIdentity = () =>
  sharedStorage.remove(SHARED_IDENTITY_SUFFIX).catch(() => false);

// Reset the in-memory cache (used after a "Switch identity" reset).
export const _resetShellCache = () => {
  cachedShell = null;
};
