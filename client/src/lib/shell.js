// Shell host detection + shared identity contract.
//
// Hey is a capsule that can be hosted by different desktop shells. We
// treat hey-home as the canonical host; stock `home` works but is a
// degraded fallback. This file owns the cross-shell contract:
//
//   /api/localhost/Users/self/.AppData/SystemServices/Shell/active.json
//     { name, version, writtenAt }   ← whichever shell is "active"
//       writes this on boot. Hey reads it to know its host.
//
//   /api/localhost/Users/self/.AppData/Identity/profile.json
//     { name, didKey, recoveryKeyHash, passkeys, createdAt, createdBy }
//       ← shared identity used by both hey-home and the Hey app, so
//         the user doesn't sign up twice.

const SHELL_MARKER_PATH =
  "/api/localhost/Users/self/.AppData/SystemServices/Shell/active.json";
const SHARED_IDENTITY_PATH =
  "/api/localhost/Users/self/.AppData/Identity/profile.json";

const safeGetJson = async (path) => {
  try {
    const r = await fetch(path, { credentials: "include" });
    if (r.status === 404) return null;
    if (!r.ok) return null;
    return await r.json();
  } catch (_) {
    return null;
  }
};

const safePutJson = async (path, value) => {
  try {
    const r = await fetch(path, {
      method: "PUT",
      credentials: "include",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(value),
    });
    return r.ok;
  } catch (_) {
    return false;
  }
};

// ── Shell detection ────────────────────────────────────────────────

let cachedShell = null;

export const detectShell = async () => {
  if (cachedShell) return cachedShell;

  // Primary: marker file written by shells that opt in (hey-home does).
  const marker = await safeGetJson(SHELL_MARKER_PATH);
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

export const readSharedIdentity = () => safeGetJson(SHARED_IDENTITY_PATH);

export const writeSharedIdentity = (profile) =>
  safePutJson(SHARED_IDENTITY_PATH, profile);

// Reset the in-memory cache (used after a "Switch identity" reset).
export const _resetShellCache = () => {
  cachedShell = null;
};
