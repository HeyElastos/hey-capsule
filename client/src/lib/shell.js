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

// Subpath-aware prefix — kept in sync with lib/runtime.js's API_BASE so
// fetch calls below resolve to /elastos/api/... under YunoHost mounts.
const API_BASE = (() => {
  if (typeof window === "undefined") return "";
  const m = window.location.pathname.match(/^(.*?)\/apps\/[^/]+\//);
  return m ? m[1] : "";
})();

// Runtime API session token read from the launched-capsule URL. Same
// recipe as lib/runtime.js — must be present as Authorization: Bearer or
// the runtime's auth_middleware returns 401 before the handler runs.
const RUNTIME_TOKEN_KEY = "hey-runtime-token";
const RUNTIME_TOKEN = (() => {
  if (typeof window === "undefined") return null;
  try {
    const fromUrl = new URLSearchParams(window.location.search).get("runtime_token");
    if (fromUrl) {
      sessionStorage.setItem(RUNTIME_TOKEN_KEY, fromUrl);
      return fromUrl;
    }
    return sessionStorage.getItem(RUNTIME_TOKEN_KEY);
  } catch {
    return null;
  }
})();
const bearerHeaders = () =>
  RUNTIME_TOKEN ? { Authorization: `Bearer ${RUNTIME_TOKEN}` } : {};

const SHELL_MARKER_PATH =
  `${API_BASE}/api/localhost/Users/self/.AppData/SystemServices/Shell/active.json`;
const SHARED_IDENTITY_PATH =
  `${API_BASE}/api/localhost/Users/self/.AppData/Identity/profile.json`;

// Storage endpoints require an X-Capability-Token in addition to the
// session Bearer. Acquire one for the (resource, action) pair via the
// runtime's auto-grant flow — the manifest declares Identity/* and
// SystemServices/Shell/* under permissions.storage so the runtime
// auto-grants immediately. Caller path is /elastos/api/localhost/...;
// the runtime's permission system uses localhost:// URIs.
import { getCapabilityToken } from "./runtime";

const pathToResource = (apiPath) =>
  "localhost://" + apiPath.replace(/^.*?\/api\/localhost\//, "");

const safeGetJson = async (path) => {
  try {
    const resource = pathToResource(path);
    const cap = await getCapabilityToken(resource, "read");
    const r = await fetch(path, {
      credentials: "include",
      headers: { ...bearerHeaders(), "X-Capability-Token": cap },
    });
    if (r.status === 404) return null;
    if (!r.ok) return null;
    return await r.json();
  } catch (_) {
    return null;
  }
};

const safePutJson = async (path, value) => {
  try {
    const resource = pathToResource(path);
    const cap = await getCapabilityToken(resource, "write");
    const r = await fetch(path, {
      method: "PUT",
      credentials: "include",
      headers: {
        "Content-Type": "application/json",
        ...bearerHeaders(),
        "X-Capability-Token": cap,
      },
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
