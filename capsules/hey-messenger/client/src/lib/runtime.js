// Runtime API client — Hey Messenger's browser-side wrapper around the
// Elastos Runtime's HTTP surface.
//
// This file is the SINGLE adapter between Hey Messenger and the runtime.
// Everything downstream (api/*, lib/*, components) talks only to the
// public exports here — when upstream rev's, this is the only file that
// needs editing. See the same architecture contract as Hey Social:
//   .../elastos-runtime_ynh/docs/HEY_MODULAR_ARCHITECTURE.md
//
// Storage strategy mirrors Hey Social: try the patch-0002 route first
// (/api/apps/hey-messenger/storage/*) and fall back to upstream-native
// /api/localhost/Users/self/* on 401/403/404. Mode is memoized in
// sessionStorage so subsequent calls skip the probe. Both shapes land
// at the same on-disk file under the launching user's principal root,
// so writes survive the runtime flipping between them.
//
// Surfaces: peer.* (Carrier), blobs.* (iroh-blobs), docs.* (CRDT,
// stubbed), webrtcSignal.* (Phase 5 stub), did.*, storage.*,
// sharedStorage.* (for cross-capsule .AppData/Identity sharing).

// Install base derived from the iframe's URL (e.g. "/elastos" when the
// runtime is mounted under a YunoHost subpath, "" when at root).
const API_BASE = (() => {
  if (typeof window === "undefined") return "";
  const m = window.location.pathname.match(/^(.*?)\/apps\/[^/]+\//);
  return m ? m[1] : "";
})();
export const apiUrl = (path) => API_BASE + path;

const PROVIDER_BASE = `${API_BASE}/api/provider`;
// Per-capsule namespace for files inside this app's "private" storage —
// becomes the first URL segment under the patch-0002 route, and lives
// inside .AppData/LocalHost/ on the legacy route.
const PRIVATE_NAMESPACE = "HeyMessenger";

// ─── Runtime session token ─────────────────────────────────────────
// Embedded by the home gateway as ?runtime_token=… on first launch.
// Cached in sessionStorage so react-router navigations don't drop it.

const RUNTIME_TOKEN_KEY = "hey-messenger-runtime-token";
const HOME_LAUNCH_TOKEN_KEY = "hey-messenger-home-launch-token";
const CAPSULE_ID = "hey-messenger";

// v0.3 auth: URL ships a home_token launch envelope, NOT a Bearer.
// Exchange it at POST /api/apps/hey-messenger/runtime-token for a
// real session bearer (added by scripts/patches/0001-capsule-runtime-
// token.patch). All API calls use the exchanged bearer. Same flow
// Hey Social uses.

let HOME_LAUNCH_TOKEN = (() => {
  if (typeof window === "undefined") return null;
  try {
    const params = new URLSearchParams(window.location.search);
    const fromUrl = params.get("home_token") || params.get("runtime_token");
    if (fromUrl) {
      const prev = sessionStorage.getItem(HOME_LAUNCH_TOKEN_KEY);
      if (prev && prev !== fromUrl) {
        sessionStorage.removeItem("hey-messenger-capability-tokens");
        sessionStorage.removeItem(RUNTIME_TOKEN_KEY);
      }
      sessionStorage.setItem(HOME_LAUNCH_TOKEN_KEY, fromUrl);
      return fromUrl;
    }
    return sessionStorage.getItem(HOME_LAUNCH_TOKEN_KEY);
  } catch {
    return null;
  }
})();

let RUNTIME_TOKEN = (() => {
  if (typeof window === "undefined") return null;
  try { return sessionStorage.getItem(RUNTIME_TOKEN_KEY); }
  catch { return null; }
})();

export const bearerReady = (async () => {
  if (RUNTIME_TOKEN) return true;
  if (!HOME_LAUNCH_TOKEN || typeof window === "undefined") return false;
  try {
    const resp = await fetch(apiUrl(`/api/apps/${CAPSULE_ID}/runtime-token`), {
      method: "POST",
      credentials: "include",
      headers: {
        "Content-Type": "application/json",
        "x-elastos-home-token": HOME_LAUNCH_TOKEN,
      },
      body: "{}",
    });
    if (!resp.ok) {
      console.warn(`[hey-messenger] runtime-token exchange failed: ${resp.status}`);
      return false;
    }
    const data = await resp.json();
    if (data && typeof data.token === "string" && data.token) {
      RUNTIME_TOKEN = data.token;
      try { sessionStorage.setItem(RUNTIME_TOKEN_KEY, RUNTIME_TOKEN); } catch (_) {}
      return true;
    }
    return false;
  } catch (err) {
    console.warn("[hey-messenger] runtime-token exchange error:", err);
    return false;
  }
})();

const TOKEN_STORE_KEY = "hey-messenger-capability-tokens";

const bearerHeaders = () =>
  RUNTIME_TOKEN ? { Authorization: `Bearer ${RUNTIME_TOKEN}` } : {};

// Launch envelope header for the patch-0002 storage route. That route
// does NOT go through the bearer middleware; it auths directly off the
// launch envelope.
const launchEnvelopeHeaders = () =>
  HOME_LAUNCH_TOKEN ? { "x-elastos-home-token": HOME_LAUNCH_TOKEN } : {};

const loadTokenStore = () => {
  try { return JSON.parse(sessionStorage.getItem(TOKEN_STORE_KEY) || "{}"); }
  catch { return {}; }
};
const saveTokenStore = (m) => {
  try { sessionStorage.setItem(TOKEN_STORE_KEY, JSON.stringify(m)); } catch {}
};

const tokenCache = loadTokenStore();
const cacheKey = (resource, action) => `${action}::${resource}`;

let fallbackToken = "capsule-session";
export const setSessionToken = (token) => {
  fallbackToken = token || "capsule-session";
};

const tokenForResource = (resource, action = "write") =>
  tokenCache[cacheKey(resource, action)] || fallbackToken;

const schemeToResource = (scheme) => `elastos://${scheme}/*`;

const requestCapabilityToken = async (resource, action = "write") => {
  // Block until the home_token→bearer exchange has resolved (or
  // proven there's no envelope to exchange). Without this the first
  // capability request fires before Authorization: Bearer is set
  // and 401s on v0.3.
  await bearerReady;
  const post = await fetch(apiUrl("/api/capability/request"), {
    method: "POST",
    credentials: "include",
    headers: { "Content-Type": "application/json", ...bearerHeaders() },
    body: JSON.stringify({ resource, action }),
  });
  if (!post.ok) throw new RuntimeError(`capability/request HTTP ${post.status}`);
  const initial = await post.json();
  if (initial.status === "granted" && initial.token) return initial.token;
  if (initial.status === "auto_denied" || initial.status === "denied") return null;
  if (initial.status !== "pending" || !initial.request_id) {
    throw new RuntimeError(`capability/request unexpected status: ${initial.status}`);
  }
  const delays = [200, 400, 800, 1500, 2000];
  const deadline = Date.now() + 30_000;
  let i = 0;
  while (Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, delays[Math.min(i, delays.length - 1)]));
    i++;
    const r = await fetch(
      apiUrl(`/api/capability/request/${encodeURIComponent(initial.request_id)}`),
      { credentials: "include", headers: bearerHeaders() }
    );
    if (!r.ok) continue;
    const status = await r.json();
    if (status.status === "granted" && status.token) return status.token;
    if (status.status === "denied" || status.status === "expired") return null;
  }
  return null;
};

export const getCapabilityToken = async (resource, action = "write") => {
  const key = cacheKey(resource, action);
  if (tokenCache[key]) return tokenCache[key];
  try {
    const token = await requestCapabilityToken(resource, action);
    if (token) {
      tokenCache[key] = token;
      saveTokenStore(tokenCache);
      return token;
    }
  } catch (err) {
    console.warn("[hey-messenger] capability acquire failed; using fallback", err);
  }
  return fallbackToken;
};

const authHeaders = (resource, action = "write") => {
  const token = resource ? tokenForResource(resource, action) : fallbackToken;
  const headers = { ...bearerHeaders() };
  if (token) headers["X-Capability-Token"] = token;
  return headers;
};

const ensureAuthedHeaders = async (resource, action) => {
  await getCapabilityToken(resource, action);
  return authHeaders(resource, action);
};

// ─── Generic provider proxy ────────────────────────────────────────

export const providerCall = async (scheme, op, body = {}) => {
  const resource = schemeToResource(scheme);
  const headers = {
    "Content-Type": "application/json",
    ...(await ensureAuthedHeaders(resource, "write")),
  };
  const resp = await fetch(
    `${PROVIDER_BASE}/${encodeURIComponent(scheme)}/${encodeURIComponent(op)}`,
    { method: "POST", credentials: "include", headers, body: JSON.stringify(body) }
  );
  if (!resp.ok) {
    const txt = await resp.text().catch(() => "");
    throw new RuntimeError(
      `provider_call(${scheme}, ${op}) → HTTP ${resp.status}`,
      { status: resp.status, body: txt }
    );
  }
  return resp.json();
};

// ─── Peer (Carrier gossip) ─────────────────────────────────────────
// Verbatim from Hey — Carrier surface is unchanged.

export const peer = {
  joinTopic: (topic) => providerCall("peer", "gossip_join", { topic }),
  leaveTopic: (topic) => providerCall("peer", "gossip_leave", { topic }),
  publish: ({ topic, message, sender_id, ts, signature }) =>
    providerCall("peer", "gossip_send", { topic, message, sender_id, ts, signature }),
  recv: ({ topic, limit = 50, consumer_id, skip_sender_id }) =>
    providerCall("peer", "gossip_recv", {
      topic, limit, consumer_id,
      ...(skip_sender_id ? { skip_sender_id } : {}),
    }),
  listTopicPeers: (topic) => providerCall("peer", "list_topic_peers", { topic }),
  listPeers: () => providerCall("peer", "list_peers", {}),
  getTicket: () => providerCall("peer", "get_ticket", {}),
};

// ─── Blobs (iroh-blobs direct P2P) ─────────────────────────────────
// Replaces Hey's `ipfs` surface. Unlimited file size bounded only by
// local disk + receiver availability. See providers/blobs-provider/.

const toBase64 = (bytes) => {
  let bin = "";
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
  return btoa(bin);
};

export const blobs = {
  // Import a server-side filesystem path. Preferred for large files —
  // bytes never touch the HTTP body, so no nginx limit applies. Needs a
  // path the capsule can read (e.g. a path returned by the runtime's
  // file-picker that already lives under the user's storage tree).
  addPath: (path) => providerCall("blobs", "add_path", { path }),

  // Import bytes from the browser. Body is base64-encoded JSON, so this
  // hits the nginx body limit (currently 100M, ~75MB after base64
  // overhead). Use only for small files; large files should go through
  // the streaming path (Phase 1 follow-up: blobs.streamUpload).
  addBytes: async (data, filename = "file") => {
    let bytes;
    if (data instanceof Uint8Array) bytes = data;
    else if (data instanceof ArrayBuffer) bytes = new Uint8Array(data);
    else if (data instanceof Blob) bytes = new Uint8Array(await data.arrayBuffer());
    else throw new Error("blobs.addBytes: data must be Uint8Array, ArrayBuffer, or Blob");
    return providerCall("blobs", "add_bytes", {
      data_base64: toBase64(bytes), filename,
    });
  },

  // Fetch a blob given an iroh-blobs ticket. Writes to a server-side
  // destination path; returns { hash }. The receiver process must be
  // online for the sender to actually push bytes.
  fetch: (ticket, dest) => providerCall("blobs", "fetch", { ticket, dest }),

  // Re-mint a ticket for a blob we already have locally. Useful when a
  // peer asks for a fresh ticket because the original expired.
  // Phase 1 follow-up — blobs-provider currently returns an error here.
  share: (hash) => providerCall("blobs", "share", { hash }),

  // Enumerate locally-stored blobs.
  // Phase 1 follow-up — blobs-provider currently returns an error here.
  list: () => providerCall("blobs", "list", {}),

  // Unpin and GC.
  // Phase 1 follow-up.
  drop: (hash) => providerCall("blobs", "drop", { hash }),
};

// ─── Docs (iroh-docs CRDT workspace state) ─────────────────────────
// Phase 4 — surface only, provider returns "not yet implemented".

export const docs = {
  create: () => providerCall("docs", "create", {}),
  open: (doc_id) => providerCall("docs", "open", { doc_id }),
  set: (doc_id, key, value) => providerCall("docs", "set", { doc_id, key, value }),
  get: (doc_id, key) => providerCall("docs", "get", { doc_id, key }),
  list: (doc_id, prefix) => providerCall("docs", "list", { doc_id, prefix }),
  // subscribe is conceptually a long-poll; the actual shape depends on
  // whether we expose SSE or WS from the docs-provider. TBD in Phase 4.
};

// ─── WebRTC signaling ──────────────────────────────────────────────
// Phase 5 — SDP offer/answer + ICE candidates ride a per-call Carrier
// topic. This wrapper is just a thin convenience over peer.publish/recv
// for that topic. Media itself is browser-to-browser WebRTC.

export const webrtcSignal = {
  topicFor: (call_id) => `hey-msg/v0/call/${call_id}/signal`,
  // Publish helper — caller signs the event with createSignedEvent first.
  send: (signedEvent, call_id) =>
    peer.publish({
      topic: webrtcSignal.topicFor(call_id),
      message: JSON.stringify(signedEvent),
      sender_id: signedEvent.sender_did,
      ts: signedEvent.ts,
      signature: signedEvent.signature,
    }),
  recv: ({ call_id, consumer_id, limit = 50, skip_sender_id }) =>
    peer.recv({ topic: webrtcSignal.topicFor(call_id), consumer_id, limit, skip_sender_id }),
};

// ─── DID provider ──────────────────────────────────────────────────

export const did = {
  resolve: (didStr) => providerCall("did", "resolve", { did: didStr }),
};

// ─── Storage adapter ───────────────────────────────────────────────
//
// One dispatch helper, two route shapes. Mirrors Hey Social — see
// that file for the long-form explanation. Working shape is detected
// on first call and memoized in sessionStorage.
//
// Path semantics:
//   storage.readJson("threads.json")
//     →  "HeyMessenger/threads.json"            (per-capsule namespace)
//   sharedStorage.readJson(".AppData/Identity/profile.json")
//     →  ".AppData/Identity/profile.json"       (cross-capsule)

const ROUTE_MODE_KEY = "hey-messenger-storage-route-mode";

let storageRouteMode = (() => {
  if (typeof window === "undefined") return null;
  try { return sessionStorage.getItem(ROUTE_MODE_KEY) || null; } catch { return null; }
})();

const setRouteMode = (mode) => {
  storageRouteMode = mode;
  try { sessionStorage.setItem(ROUTE_MODE_KEY, mode); } catch {}
};

const clean = (s) => (s || "").replace(/^\/+/, "");

const buildRequest = (mode, suffix) => {
  const s = clean(suffix);
  if (mode === "patch-0002") {
    return {
      url: `${API_BASE}/api/apps/${CAPSULE_ID}/storage/${s}`,
      headers: launchEnvelopeHeaders(),
    };
  }
  const legacySuffix = s.startsWith(`${PRIVATE_NAMESPACE}/`)
    ? `.AppData/LocalHost/${s}`
    : s;
  return {
    url: `${API_BASE}/api/localhost/Users/self/${legacySuffix}`,
    headers: bearerHeaders(),
  };
};

const dispatchStorage = async (suffix, init = {}) => {
  await bearerReady.catch(() => false);

  const attempt = async (mode) => {
    const { url, headers } = buildRequest(mode, suffix);
    const finalHeaders = { ...init.headers, ...headers };
    return fetch(url, { ...init, credentials: "include", headers: finalHeaders });
  };

  if (storageRouteMode) {
    return attempt(storageRouteMode);
  }
  let resp = await attempt("patch-0002");
  if (resp.status === 401 || resp.status === 403 || resp.status === 404) {
    const legacyResp = await attempt("legacy");
    if (legacyResp.status < 500 && legacyResp.status !== 401 && legacyResp.status !== 403) {
      setRouteMode("legacy");
      return legacyResp;
    }
    setRouteMode("patch-0002");
    return resp;
  }
  setRouteMode("patch-0002");
  return resp;
};

export const storage = {
  readJson: async (path) => {
    const resp = await dispatchStorage(`${PRIVATE_NAMESPACE}/${clean(path)}`);
    if (resp.status === 404) return null;
    if (!resp.ok) throw new RuntimeError(`storage GET ${path}: HTTP ${resp.status}`);
    return resp.json();
  },
  writeJson: async (path, value) => {
    const resp = await dispatchStorage(`${PRIVATE_NAMESPACE}/${clean(path)}`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(value),
    });
    if (!resp.ok) {
      const txt = await resp.text().catch(() => "");
      throw new RuntimeError(`storage PUT ${path}: HTTP ${resp.status}`,
        { status: resp.status, body: txt });
    }
    return true;
  },
  remove: async (path) => {
    const resp = await dispatchStorage(`${PRIVATE_NAMESPACE}/${clean(path)}`, {
      method: "DELETE",
    });
    if (!resp.ok && resp.status !== 404)
      throw new RuntimeError(`storage DELETE ${path}: HTTP ${resp.status}`);
    return true;
  },
  list: async (path) => {
    const suffix = `${PRIVATE_NAMESPACE}/${clean(path)}`;
    await dispatchStorage(suffix);
    const { url, headers } = buildRequest(storageRouteMode || "patch-0002", suffix);
    const resp = await fetch(`${url}?list=true`, { credentials: "include", headers });
    if (resp.status === 404) return [];
    if (!resp.ok) throw new RuntimeError(`storage LIST ${path}: HTTP ${resp.status}`);
    return resp.json();
  },
};

// Shared-namespace storage. Used for cross-capsule paths like
// .AppData/Identity/profile.json that other capsules under the same
// principal can read (notably Hey Social and the home shell). Suffix
// is taken as-is, no per-capsule prefix added.
export const sharedStorage = {
  readJson: async (suffix) => {
    const resp = await dispatchStorage(suffix);
    if (resp.status === 404) return null;
    if (!resp.ok) throw new RuntimeError(`sharedStorage GET ${suffix}: HTTP ${resp.status}`);
    return resp.json();
  },
  writeJson: async (suffix, value) => {
    const resp = await dispatchStorage(suffix, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(value),
    });
    if (!resp.ok) {
      const txt = await resp.text().catch(() => "");
      throw new RuntimeError(`sharedStorage PUT ${suffix}: HTTP ${resp.status}`,
        { status: resp.status, body: txt });
    }
    return true;
  },
  remove: async (suffix) => {
    const resp = await dispatchStorage(suffix, { method: "DELETE" });
    if (!resp.ok && resp.status !== 404)
      throw new RuntimeError(`sharedStorage DELETE ${suffix}: HTTP ${resp.status}`);
    return true;
  },
};

export const _setStorageRouteMode = (mode) => {
  if (mode !== "patch-0002" && mode !== "legacy" && mode !== null) {
    throw new Error(`_setStorageRouteMode: invalid mode ${mode}`);
  }
  if (mode === null) {
    storageRouteMode = null;
    try { sessionStorage.removeItem(ROUTE_MODE_KEY); } catch {}
  } else {
    setRouteMode(mode);
  }
};

// ─── Session helpers ───────────────────────────────────────────────
//
// Upstream-canonical "who am I" for the current launch. Same shape as
// Hey Social's session.current() — boot adoption uses it to detect
// the runtime user without prompting for a recovery key or passkey.

const authHeaderForSession = () =>
  RUNTIME_TOKEN ? { Authorization: `Bearer ${RUNTIME_TOKEN}` } : {};

export const session = {
  current: async () => {
    await bearerReady.catch(() => false);
    const r = await fetch(apiUrl("/api/session"), {
      credentials: "include",
      headers: authHeaderForSession(),
    });
    return r.ok ? r.json() : null;
  },
};

// ─── Error type ────────────────────────────────────────────────────

export class RuntimeError extends Error {
  constructor(message, meta = {}) {
    super(message);
    this.name = "RuntimeError";
    Object.assign(this, meta);
  }
}
