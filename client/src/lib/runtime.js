// Runtime API client — Hey's browser-side wrapper around the Elastos Runtime's
// HTTP surface. Replaces axios calls to Hey's Express backend when the SPA
// is running inside a WASM capsule.
//
// Three endpoint families matter:
//
//   POST /api/provider/:scheme/:op  — capability-gated proxy to any provider.
//     Carrier ops live under :scheme=peer (gossip_send/recv/join/leave/etc.),
//     IPFS under :scheme=ipfs (add_bytes/cat/pin/etc.),
//     DID resolve+sign+verify under :scheme=did.
//
//   GET/PUT/DELETE /api/apps/hey-social/storage/*path  — principal-scoped
//     storage CRUD for third-party app capsules. Each PUT lands at
//     localhost://Users/<sha256(principal)[:24]>/<path> on disk, so the
//     filesystem root is shared with any other capsule launched under the
//     same user (cross-capsule shared identity). Auth is the launch
//     envelope (x-elastos-home-token header), NOT the runtime bearer.
//     v0.3 added this via scripts/patches/0002-capsule-principal-storage.patch
//     in the YunoHost build; v0.3's stock /api/localhost/Users/* rejects
//     third-party callers.
//
//   POST /api/apps/hey-social/runtime-token  — trades the home_token launch
//     envelope for a real session bearer. Required for /api/capability/*
//     and /api/provider/* which still go through the bearer-gated middleware.
//     Added by scripts/patches/0001-capsule-runtime-token.patch.
//
// Auth: capability tokens via X-Capability-Token header for provider calls;
// launch envelope via x-elastos-home-token header for storage calls.

// Install base derived from the iframe's URL (e.g. "/elastos" when the
// runtime is mounted under YunoHost subpath, "" when at root). The same
// pattern lives in capsules/home/browser/{hey-runtime,shell-core}.js;
// keep them in sync.
const API_BASE = (() => {
  if (typeof window === "undefined") return "";
  const m = window.location.pathname.match(/^(.*?)\/apps\/[^/]+\//);
  return m ? m[1] : "";
})();
export const apiUrl = (path) => API_BASE + path;

// Pulled out so the capsule id is in one place — must match the route name
// used by the gateway when serving /apps/<name>/ and the :capsule param on
// /api/apps/:capsule/{runtime-token,storage}.
const CAPSULE_ID = "hey-social";

const STORAGE_BASE = `${API_BASE}/api/apps/${CAPSULE_ID}/storage/Hey`;
const SHARED_STORAGE_BASE = `${API_BASE}/api/apps/${CAPSULE_ID}/storage`;
const PROVIDER_BASE = `${API_BASE}/api/provider`;

// ── Capability tokens ──────────────────────────────────────────────
// The runtime's POST /api/capability/request returns either:
//   { status: "granted",  token }      ← auto-granted for trusted apps
//   { status: "pending",  request_id } ← user must approve in shell;
//                                        poll GET /api/capability/request/:id
//   { status: "auto_denied" | "denied", reason }
//
// Tokens are bearer-style; we send them via the X-Capability-Token header.
// We cache acquired tokens in sessionStorage keyed by resource so they
// survive page navigation within a session but not a full sign-out.

const RUNTIME_TOKEN_KEY = "hey-runtime-token";
const HOME_LAUNCH_TOKEN_KEY = "hey-home-launch-token";

// v0.3 auth architecture:
//   URL  ?home_token=<launch-envelope>  (signed by gateway, scoped to "hey-social")
//   ─→ POST /api/apps/hey-social/runtime-token
//       Header: x-elastos-home-token: <launch-envelope>
//   ─→ Response: { token: <session-bearer> }
//   ─→ Use the session bearer as Authorization: Bearer for everything else
//
// The launch envelope is NOT a Bearer; v0.3's storage / capability /
// provider handlers want a session token minted via /api/auth/attach.
// Our YunoHost build patches upstream to add /api/apps/:capsule/
// runtime-token which trades launch envelope → session bearer (see
// scripts/patches/0001-capsule-runtime-token.patch). When upstream
// merges the equivalent, this client code stays valid.

// Pull the launch envelope from URL (fresh launch) or sessionStorage
// (subsequent navigations within the SPA). Falls back to legacy
// runtime_token name for v0.2 hosts.
export const HOME_LAUNCH_TOKEN = (() => {
  if (typeof window === "undefined") return null;
  try {
    const params = new URLSearchParams(window.location.search);
    const fromUrl = params.get("home_token") || params.get("runtime_token");
    if (fromUrl) {
      const prev = sessionStorage.getItem(HOME_LAUNCH_TOKEN_KEY);
      if (prev && prev !== fromUrl) {
        // New launch envelope means upstream restarted or this is a
        // fresh launch — drop cached tokens since they were bound to a
        // previous session.
        sessionStorage.removeItem("hey-capability-tokens");
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

// The actual session bearer used for /api/capability/* and /api/provider/*
// calls. Starts null; bearerReady populates it via the exchange endpoint.
// We DO NOT read this from URL — only from the exchange endpoint response.
let RUNTIME_TOKEN = (() => {
  if (typeof window === "undefined") return null;
  try {
    return sessionStorage.getItem(RUNTIME_TOKEN_KEY);
  } catch {
    return null;
  }
})();

// Boot handshake — exchange the home_token launch envelope for a
// real session bearer. Resolves to true once RUNTIME_TOKEN is set
// (either freshly minted this tick, or already in sessionStorage
// from a prior load within this session). Resolves to false on
// failure so call sites can decide whether to surface a 401.
//
// Capability / provider helpers below await this before issuing any
// request. Storage helpers do NOT — they use the launch envelope
// directly via the /api/apps/:capsule/storage/* route.
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
      console.warn(`[hey-social] runtime-token exchange failed: ${resp.status}`);
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
    console.warn("[hey-social] runtime-token exchange error:", err);
    return false;
  }
})();

const TOKEN_STORE_KEY = "hey-capability-tokens";

// Authorization: Bearer <runtime_token> on every runtime API call —
// the runtime's auth_middleware rejects requests without this with 401.
const bearerHeaders = () =>
  RUNTIME_TOKEN ? { Authorization: `Bearer ${RUNTIME_TOKEN}` } : {};

// Launch envelope header for the /api/apps/:capsule/storage/* route.
// That route does NOT go through the bearer middleware; it auths
// directly off the launch envelope.
const launchEnvelopeHeaders = () =>
  HOME_LAUNCH_TOKEN ? { "x-elastos-home-token": HOME_LAUNCH_TOKEN } : {};

const loadTokenStore = () => {
  try {
    return JSON.parse(sessionStorage.getItem(TOKEN_STORE_KEY) || "{}");
  } catch (_) { return {}; }
};
const saveTokenStore = (m) => {
  try { sessionStorage.setItem(TOKEN_STORE_KEY, JSON.stringify(m)); } catch (_) {}
};

const tokenCache = loadTokenStore();
const cacheKey = (resource, action) => `${action}::${resource}`;

// Shell sessions don't need a token; the runtime trusts them. For
// dev/local mode the placeholder is fine. When we acquire a real
// token we replace the placeholder for that resource.
let fallbackToken = "capsule-session";

export const setSessionToken = (token) => {
  fallbackToken = token || "capsule-session";
};

// Resource+action → token (or fallback). The runtime's capability
// validator does exact-action match (Read tokens don't cover Write or
// vice versa) so the cache must distinguish actions.
const tokenForResource = (resource, action = "write") =>
  tokenCache[cacheKey(resource, action)] || fallbackToken;

const schemeToResource = (scheme) => {
  // Conservative default: ask for write on the whole scheme namespace.
  // Real grants are usually narrower; that's fine, the runtime returns
  // a token scoped to whatever the policy allowed.
  return `elastos://${scheme}/*`;
};

// Acquire a capability token for the given resource + action. Returns
// the token string, null if denied, or throws on transport error.
const requestCapabilityToken = async (resource, action = "write") => {
  // Wait for the launch-envelope-to-Bearer handshake (or its failure) so
  // the request below carries Authorization: Bearer when possible.
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

  // Poll for grant. The shell renders the pending request; user clicks
  // Grant. Backoff: 200ms, 400, 800, 1500, then 2000 forever, max 30s.
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
  return null; // timed out
};

// Public helper: get a token for a (resource, action) pair, cached.
// Idempotent. Auto-acquires from /api/capability/request if missing
// (the runtime auto-grants any resource declared in the capsule's
// manifest, so this is usually a single round-trip on first use).
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
    console.warn("[hey] capability acquire failed; using fallback", err);
  }
  return fallbackToken;
};

const authHeaders = (resource, action = "write") => {
  const token = resource ? tokenForResource(resource, action) : fallbackToken;
  const headers = { ...bearerHeaders() };
  if (token) headers["X-Capability-Token"] = token;
  return headers;
};

// One-call helper: ensure we hold a capability for (resource, action),
// then return the X-Capability-Token + Authorization header pair ready
// to spread into a fetch options.headers object. Used by every provider
// call so the validator's exact-action match always finds a token.
const ensureAuthedHeaders = async (resource, action) => {
  await getCapabilityToken(resource, action);
  return authHeaders(resource, action);
};

// ─── Provider calls ────────────────────────────────────────────────

// Generic provider-proxy call: POST /api/provider/<scheme>/<op> with JSON body.
// Returns the parsed JSON response. Throws on HTTP error.
export const providerCall = async (scheme, op, body = {}) => {
  // Auto-acquire a capability for this provider scheme on miss — the
  // runtime auto-grants any resource declared in the capsule manifest,
  // so this is a single round-trip the first time and cached after.
  // Without this, providerCall sent the literal 'capsule-session'
  // placeholder and the runtime returned 403.
  const resource = schemeToResource(scheme);
  const headers = {
    "Content-Type": "application/json",
    ...(await ensureAuthedHeaders(resource, "write")),
  };
  const resp = await fetch(`${PROVIDER_BASE}/${encodeURIComponent(scheme)}/${encodeURIComponent(op)}`, {
    method: "POST",
    credentials: "include",
    headers,
    body: JSON.stringify(body),
  });
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

export const peer = {
  joinTopic: (topic) => providerCall("peer", "gossip_join", { topic }),
  leaveTopic: (topic) => providerCall("peer", "gossip_leave", { topic }),
  publish: ({ topic, message, sender_id, ts, signature }) =>
    providerCall("peer", "gossip_send", {
      topic,
      message,
      sender_id,
      ts,
      signature,
    }),
  recv: ({ topic, limit = 50, consumer_id, skip_sender_id }) =>
    providerCall("peer", "gossip_recv", {
      topic,
      limit,
      consumer_id,
      ...(skip_sender_id ? { skip_sender_id } : {}),
    }),
  listTopicPeers: (topic) => providerCall("peer", "list_topic_peers", { topic }),
  listPeers: () => providerCall("peer", "list_peers", {}),
  getTicket: () => providerCall("peer", "get_ticket", {}),
};

// ─── IPFS (media storage via Kubo) ─────────────────────────────────

const toBase64 = (bytes) => {
  let bin = "";
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
  return btoa(bin);
};

const fromBase64 = (b64) => {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
};

export const ipfs = {
  // Add bytes (browser Blob/File or Uint8Array). Returns { cid, ... }.
  addBytes: async (data, filename = "file", pin = true) => {
    let bytes;
    if (data instanceof Uint8Array) bytes = data;
    else if (data instanceof ArrayBuffer) bytes = new Uint8Array(data);
    else if (data instanceof Blob) bytes = new Uint8Array(await data.arrayBuffer());
    else throw new Error("ipfs.addBytes: data must be Uint8Array, ArrayBuffer, or Blob");
    return providerCall("ipfs", "add_bytes", {
      data: toBase64(bytes),
      filename,
      pin,
    });
  },

  // Add many files as a directory. Returns the directory CID.
  // files = [{ path: "name.ext", data: Blob|Uint8Array }, ...]
  addDirectory: async (files, pin = true) => {
    const dirFiles = await Promise.all(
      files.map(async (f) => {
        let bytes;
        if (f.data instanceof Uint8Array) bytes = f.data;
        else if (f.data instanceof Blob)
          bytes = new Uint8Array(await f.data.arrayBuffer());
        else throw new Error("ipfs.addDirectory: each file.data must be Uint8Array or Blob");
        return { path: f.path, data: toBase64(bytes) };
      })
    );
    return providerCall("ipfs", "add_directory", { files: dirFiles, pin });
  },

  // Read bytes for a CID. Returns Uint8Array.
  getBytes: async (cid, path) => {
    const resp = await providerCall("ipfs", "get_bytes", {
      cid,
      ...(path ? { path } : {}),
    });
    const b64 = resp?.data?.data;
    if (!b64) throw new RuntimeError(`get_bytes(${cid}): no data in response`);
    return fromBase64(b64);
  },

  // Construct a URL the IPFS gateway serves directly. Useful for <img src>.
  // nginx proxies /<API_BASE>/ipfs/<CID> to Kubo's gateway on :8080 with
  // no auth required — CIDs are content-addressed, so possession of the
  // CID is itself the access token. This avoids the <img>-auth problem
  // where direct image-element fetches can't carry Authorization or
  // X-Capability-Token headers and 401 against the runtime API.
  gatewayUrl: (cid, path) => {
    const suffix = path ? `/${path.replace(/^\/+/, "")}` : "";
    return `${API_BASE}/ipfs/${encodeURIComponent(cid)}${suffix}`;
  },

  pin: (cid) => providerCall("ipfs", "pin", { cid }),
  unpin: (cid) => providerCall("ipfs", "unpin", { cid }),
  ls: (cid) => providerCall("ipfs", "ls", { cid }),
  health: () => providerCall("ipfs", "health", {}),
};

// ─── Transcoder provider (image / video / voice via ffmpeg) ─────────
//
// Wraps the hey-transcoder capsule. Each op base64s the input, hands it
// off to the capsule, and base64-decodes the response. processForUpload
// is the typical entry point: it inspects the Blob's MIME type, calls
// the right transcode op, and falls through to the original blob if
// the capsule isn't installed or ffmpeg can't handle the input.

const blobToB64 = async (blob) =>
  toBase64(new Uint8Array(await blob.arrayBuffer()));

export const transcoder = {
  transcodeImage: async (blob, opts = {}) =>
    providerCall("hey-transcoder", "transcode_image", {
      data: await blobToB64(blob),
      target_format: opts.targetFormat || "webp",
      max_dim: opts.maxDim || 2048,
      quality: opts.quality || 85,
      strip_metadata: opts.stripMetadata !== false,
    }),

  transcodeVideo: async (blob, opts = {}) =>
    providerCall("hey-transcoder", "transcode_video", {
      data: await blobToB64(blob),
      target_codec: opts.targetCodec || "h264",
      max_dim: opts.maxDim || 1080,
      crf: opts.crf || 23,
      fps: opts.fps || 30,
      preset: opts.preset || "fast",
    }),

  transcodeVoice: async (blob, opts = {}) =>
    providerCall("hey-transcoder", "transcode_voice", {
      data: await blobToB64(blob),
      target_codec: opts.targetCodec || "opus",
      bitrate_k: opts.bitrateK || 64,
      normalize_lufs: opts.normalizeLufs ?? -16,
    }),

  thumbnailVideo: async (blob, opts = {}) =>
    providerCall("hey-transcoder", "thumbnail_video", {
      data: await blobToB64(blob),
      time_offset_s: opts.timeOffsetS ?? 1.0,
      max_dim: opts.maxDim || 480,
    }),

  // Convenience: inspect Blob.type, call the right transcode op, return a
  // fresh Blob ready for ipfs.addBytes. Silently passes through on any
  // failure so uploads keep working when the capsule isn't installed.
  processForUpload: async (data, hint = {}) => {
    if (!(data instanceof Blob)) {
      throw new Error("transcoder.processForUpload: expected Blob/File");
    }
    const type = (data.type || hint.type || "").toLowerCase();
    try {
      let result;
      let mediaPrefix;
      if (type.startsWith("image/")) {
        result = await transcoder.transcodeImage(data, hint);
        mediaPrefix = "image";
      } else if (type.startsWith("video/")) {
        result = await transcoder.transcodeVideo(data, hint);
        mediaPrefix = "video";
      } else if (type.startsWith("audio/")) {
        result = await transcoder.transcodeVoice(data, hint);
        mediaPrefix = "audio";
      } else {
        // Not a recognized media type — pass through unchanged.
        return { blob: data, format: null, transcoded: false };
      }
      // Provider proxy returns 200 with an error JSON body when the
      // hey-transcoder capsule isn't installed (or returns {ok:false}
      // explicitly). Validate the response shape before trusting it.
      if (!result || result.ok === false || typeof result.data !== "string") {
        throw new RuntimeError(
          (result && result.error) || "transcoder returned no data; passing through"
        );
      }
      const bytes = fromBase64(result.data);
      return {
        blob: new Blob([bytes], { type: `${mediaPrefix}/${result.format}` }),
        format: result.format,
        codec: result.codec,
        width: result.width,
        height: result.height,
        size_bytes: result.size_bytes,
        transcoded: true,
      };
    } catch (err) {
      // hey-transcoder not installed, ffmpeg error, codec missing, etc.
      // Don't fail the upload — just post the original and warn.
      console.warn("[hey] transcode failed, uploading original:", err);
      return { blob: data, format: null, transcoded: false, error: err };
    }
  },

  health: () => providerCall("hey-transcoder", "health", {}),
};

// ─── DID provider (resolve / verify any did:key) ───────────────────

export const did = {
  // Resolve any did:key — returns pubkey + DID doc. Works for both runtime-
  // issued machine DIDs and Hey-issued user DIDs (same format).
  resolve: (did) => providerCall("did", "resolve", { did }),
};

// ─── Principal-scoped storage CRUD ─────────────────────────────────
//
// Routes through /api/apps/hey-social/storage/* (patch 0002). Each
// PUT lands at localhost://Users/<sha256(principal)[:24]>/<suffix>
// on disk. Two app capsules launched under the same user share the
// same root, so cross-capsule shared paths (.AppData/Identity/*)
// work the way v0.2's /api/localhost/Users/self/* did.
//
// Path segments must not contain ".", "..", or "\". The route handler
// validates this and returns 400 if violated.

const storagePath = (relative) =>
  `${STORAGE_BASE}/${(relative || "").replace(/^\/+/, "")}`;

// Hey-private storage. Bare relative path → localhost://Users/<hash>/Hey/<path>.
// e.g. "profile.json" → localhost://Users/<hash>/Hey/profile.json
export const storage = {
  readJson: async (path) => {
    const resp = await fetch(storagePath(path), {
      credentials: "include",
      headers: launchEnvelopeHeaders(),
    });
    if (resp.status === 404) return null;
    if (!resp.ok)
      throw new RuntimeError(`storage GET ${path}: HTTP ${resp.status}`);
    return resp.json();
  },

  writeJson: async (path, value) => {
    const headers = {
      "Content-Type": "application/json",
      ...launchEnvelopeHeaders(),
    };
    const resp = await fetch(storagePath(path), {
      method: "PUT",
      credentials: "include",
      headers,
      body: JSON.stringify(value),
    });
    if (!resp.ok) {
      const txt = await resp.text().catch(() => "");
      throw new RuntimeError(`storage PUT ${path}: HTTP ${resp.status}`, {
        status: resp.status,
        body: txt,
      });
    }
    return true;
  },

  remove: async (path) => {
    const resp = await fetch(storagePath(path), {
      method: "DELETE",
      credentials: "include",
      headers: launchEnvelopeHeaders(),
    });
    if (!resp.ok && resp.status !== 404)
      throw new RuntimeError(`storage DELETE ${path}: HTTP ${resp.status}`);
    return true;
  },

  list: async (path) => {
    const resp = await fetch(`${storagePath(path)}?list=true`, {
      credentials: "include",
      headers: launchEnvelopeHeaders(),
    });
    if (resp.status === 404) return [];
    if (!resp.ok)
      throw new RuntimeError(`storage LIST ${path}: HTTP ${resp.status}`);
    return resp.json();
  },
};

// Shared-namespace storage (paths outside the Hey/ prefix). Used for
// cross-capsule files like .AppData/Identity/profile.json that the
// home shell and other capsules under the same principal can read.
// Caller passes the full suffix under the principal root.
export const sharedStorage = {
  readJson: async (suffix) => {
    const url = `${SHARED_STORAGE_BASE}/${suffix.replace(/^\/+/, "")}`;
    const resp = await fetch(url, {
      credentials: "include",
      headers: launchEnvelopeHeaders(),
    });
    if (resp.status === 404) return null;
    if (!resp.ok)
      throw new RuntimeError(`sharedStorage GET ${suffix}: HTTP ${resp.status}`);
    return resp.json();
  },

  writeJson: async (suffix, value) => {
    const url = `${SHARED_STORAGE_BASE}/${suffix.replace(/^\/+/, "")}`;
    const headers = {
      "Content-Type": "application/json",
      ...launchEnvelopeHeaders(),
    };
    const resp = await fetch(url, {
      method: "PUT",
      credentials: "include",
      headers,
      body: JSON.stringify(value),
    });
    if (!resp.ok) {
      const txt = await resp.text().catch(() => "");
      throw new RuntimeError(`sharedStorage PUT ${suffix}: HTTP ${resp.status}`, {
        status: resp.status,
        body: txt,
      });
    }
    return true;
  },

  remove: async (suffix) => {
    const url = `${SHARED_STORAGE_BASE}/${suffix.replace(/^\/+/, "")}`;
    const resp = await fetch(url, {
      method: "DELETE",
      credentials: "include",
      headers: launchEnvelopeHeaders(),
    });
    if (!resp.ok && resp.status !== 404)
      throw new RuntimeError(`sharedStorage DELETE ${suffix}: HTTP ${resp.status}`);
    return true;
  },
};

// ── Boot-time capability acquisition ──────────────────────────────
// Hey needs message on each provider it actually uses. Acquire them in
// parallel at app boot so every subsequent provider call has a real
// token in its X-Capability-Token header. Each acquire falls through
// silently to the placeholder if the runtime returns "denied" or isn't
// gating yet — so this is non-blocking and dev-mode-friendly.
//
// Note: localhost storage is no longer in the list — that route now
// auths off the launch envelope, not capability tokens.
export const acquireBootCapabilities = async () => {
  const wants = [
    { resource: "elastos://peer/*",    action: "message" },
    { resource: "elastos://ipfs/*",    action: "write" },
    { resource: "elastos://did/*",     action: "read" },
    { resource: "elastos://hey-transcoder/*", action: "execute" },
  ];
  await Promise.all(
    wants.map((w) => getCapabilityToken(w.resource, w.action).catch(() => null))
  );
};

// ─── Capability flow (operator-grant model) ────────────────────────

export const capability = {
  request: ({ resource, action }) =>
    fetch(apiUrl("/api/capability/request"), {
      method: "POST",
      credentials: "include",
      headers: { "Content-Type": "application/json", ...authHeaders() },
      body: JSON.stringify({ resource, action }),
    }).then((r) => r.json()),
  status: (id) =>
    fetch(apiUrl(`/api/capability/request/${encodeURIComponent(id)}`), {
      credentials: "include",
      headers: authHeaders(),
    }).then((r) => r.json()),
  list: () =>
    fetch(apiUrl("/api/capability/list"), {
      credentials: "include",
      headers: authHeaders(),
    }).then((r) => r.json()),
};

// ─── Session helpers ───────────────────────────────────────────────

export const session = {
  current: () =>
    fetch(apiUrl("/api/session"), {
      credentials: "include",
      headers: authHeaders(),
    }).then((r) => (r.ok ? r.json() : null)),
};

// ─── Errors ────────────────────────────────────────────────────────

export class RuntimeError extends Error {
  constructor(message, { status, body } = {}) {
    super(message);
    this.name = "RuntimeError";
    this.status = status;
    this.responseBody = body;
  }
}
