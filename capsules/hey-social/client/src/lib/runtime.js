// Runtime API client — Hey's browser-side wrapper around the Elastos Runtime's
// HTTP surface.
//
// This file is the SINGLE adapter between Hey and the runtime. Everything
// downstream of it (api/auth.js, api/chat.js, lib/vault.js, lib/shell.js,
// components) talks only to the exports here — when upstream rev's, this
// file is the only one that needs editing. See:
//   ../../../../elastos-runtime-ynh/docs/HEY_MODULAR_ARCHITECTURE.md
//
// Storage strategy (adapter pattern, version-resilient):
//
//   Hey makes one logical call (e.g. storage.readJson("profile.json"))
//   and the adapter picks the route shape that works against the runtime
//   it's actually talking to. Two shapes are supported:
//
//     1. patch-0002 route   GET/PUT/DELETE /api/apps/:capsule/storage/*
//        v0.3 + scripts/patches/0002-capsule-principal-storage.patch.
//        Auths off the launch envelope (x-elastos-home-token).
//
//     2. upstream-native    GET/PUT/DELETE /api/localhost/Users/self/*
//        v0.2 + any future upstream that restores third-party access.
//        Auths off the bearer minted by patch 0001's runtime-token
//        exchange. v0.3 stock rejects this for third-party callers via
//        storage::reject_principal_root_storage_path; patch 0002
//        exists because of that rejection.
//
//   The adapter tries (1) first, memoizes the working shape, falls back
//   to (2) on 401/403/404. Capsule.json declares permissions for BOTH
//   shapes so either works as far as the manifest check is concerned.
//
// Other endpoint families (also adapter-stable):
//
//   POST /api/provider/:scheme/:op           — capability-gated provider proxy
//   POST /api/apps/:capsule/runtime-token    — bearer exchange (patch 0001)
//   POST /api/capability/request             — capability auto-grant
//
// Auth headers used by this file:
//   - Authorization: Bearer <runtime_token>   for provider + capability calls
//   - X-Capability-Token: <cap>               for provider calls
//   - x-elastos-home-token: <launch-envelope> for the patch-0002 storage route

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

// Per-capsule sub-namespace for files inside this app's "private" storage.
// Under the patch-0002 route this becomes the first URL segment after
// /storage/; under the legacy route it lives inside .AppData/LocalHost/.
const PRIVATE_NAMESPACE = "Hey";

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

// ─── Storage adapter ───────────────────────────────────────────────
//
// One dispatch helper, two route shapes (see file header). The shape
// that works against the current runtime is detected on first call and
// memoized in sessionStorage so subsequent requests skip the probe.
//
// Both shapes resolve to the same on-disk file under the launching
// user's principal root, so a write under one shape is readable under
// the other if the runtime ever flips between them.
//
// Path semantics:
//   storage.readJson("profile.json")
//     →  "Hey/profile.json"             (private, namespaced under Hey/)
//   sharedStorage.readJson(".AppData/Identity/profile.json")
//     →  ".AppData/Identity/profile.json"  (shared with other capsules)
//
// Route translation:
//   "Hey/<file>"                  patch-0002  /api/apps/hey-social/storage/Hey/<file>
//                                 legacy      /api/localhost/Users/self/.AppData/LocalHost/Hey/<file>
//   ".AppData/<rest>"             patch-0002  /api/apps/hey-social/storage/.AppData/<rest>
//                                 legacy      /api/localhost/Users/self/.AppData/<rest>

const ROUTE_MODE_KEY = "hey-storage-route-mode";

// 'patch-0002' | 'legacy' | null (unknown — will probe on first call)
let storageRouteMode = (() => {
  if (typeof window === "undefined") return null;
  try { return sessionStorage.getItem(ROUTE_MODE_KEY) || null; } catch { return null; }
})();

const setRouteMode = (mode) => {
  storageRouteMode = mode;
  try { sessionStorage.setItem(ROUTE_MODE_KEY, mode); } catch (_) {}
};

const clean = (s) => (s || "").replace(/^\/+/, "");

// Build the URL + headers pair for a given (mode, suffix). The suffix
// is the path under the principal root: either "Hey/foo.json" (private)
// or ".AppData/Identity/profile.json" (shared) — both shapes are valid.
const buildRequest = (mode, suffix) => {
  const s = clean(suffix);
  if (mode === "patch-0002") {
    return {
      url: `${API_BASE}/api/apps/${CAPSULE_ID}/storage/${s}`,
      headers: launchEnvelopeHeaders(),
    };
  }
  // legacy: Hey/<file> → .AppData/LocalHost/Hey/<file>;
  //         .AppData/<rest> stays as .AppData/<rest>
  const legacySuffix = s.startsWith(`${PRIVATE_NAMESPACE}/`)
    ? `.AppData/LocalHost/${s}`
    : s;
  return {
    url: `${API_BASE}/api/localhost/Users/self/${legacySuffix}`,
    headers: bearerHeaders(),
  };
};

// Dispatch a single storage request. On the first call (mode === null)
// we probe patch-0002 first since that's the v0.3+ shape; on 401/403/404
// we fall back to legacy and remember the working mode.
const dispatchStorage = async (suffix, init = {}) => {
  // Bearer fallback path needs the runtime token; wait for the exchange.
  await bearerReady.catch(() => false);

  const attempt = async (mode) => {
    const { url, headers } = buildRequest(mode, suffix);
    const finalHeaders = { ...init.headers, ...headers };
    return fetch(url, { ...init, credentials: "include", headers: finalHeaders });
  };

  if (storageRouteMode) {
    return attempt(storageRouteMode);
  }
  // Try patch-0002 first.
  let resp = await attempt("patch-0002");
  if (resp.status === 401 || resp.status === 403 || resp.status === 404) {
    // Could be "patch not applied" OR "this object doesn't exist on the
    // patch-0002 route." Probe legacy with the same suffix; if legacy
    // responds 2xx/404, lock to legacy. If legacy also errors with
    // 401/403, stay on patch-0002 (likely an auth issue, not a route
    // issue) and let the caller see the original error.
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

// Public storage surface, identical across both route shapes. Path
// argument is the suffix under the per-capsule namespace (e.g.
// "profile.json", "posts/by-id/<id>.json").
export const storage = {
  readJson: async (path) => {
    const resp = await dispatchStorage(`${PRIVATE_NAMESPACE}/${clean(path)}`);
    if (resp.status === 404) return null;
    if (!resp.ok)
      throw new RuntimeError(`storage GET ${path}: HTTP ${resp.status}`);
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
      throw new RuntimeError(`storage PUT ${path}: HTTP ${resp.status}`, {
        status: resp.status,
        body: txt,
      });
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
    // Both routes support ?list=true at the directory URL.
    const suffix = `${PRIVATE_NAMESPACE}/${clean(path)}`;
    // Build the URL via the dispatcher's helper so the route mode is
    // honored; we can't pass query params through buildRequest cleanly,
    // so we do the dispatch + then append the query string.
    const probe = await dispatchStorage(suffix);
    if (probe.status === 404) {
      // If the listing returns 404 against an empty directory, fall through.
    }
    // Re-issue with ?list=true on the same route now that mode is set.
    const { url, headers } = buildRequest(storageRouteMode || "patch-0002", suffix);
    const resp = await fetch(`${url}?list=true`, {
      credentials: "include",
      headers,
    });
    if (resp.status === 404) return [];
    if (!resp.ok)
      throw new RuntimeError(`storage LIST ${path}: HTTP ${resp.status}`);
    return resp.json();
  },
};

// Shared-namespace storage. Used for cross-capsule paths like
// .AppData/Identity/profile.json that other capsules under the same
// principal can read. Suffix is taken as-is (no Hey/ prefix).
export const sharedStorage = {
  readJson: async (suffix) => {
    const resp = await dispatchStorage(suffix);
    if (resp.status === 404) return null;
    if (!resp.ok)
      throw new RuntimeError(`sharedStorage GET ${suffix}: HTTP ${resp.status}`);
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
      throw new RuntimeError(`sharedStorage PUT ${suffix}: HTTP ${resp.status}`, {
        status: resp.status,
        body: txt,
      });
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

// Test/debug helper — force a specific route mode and skip probing.
// Intended for unit tests and operator debugging; production code should
// never call this.
export const _setStorageRouteMode = (mode) => {
  if (mode !== "patch-0002" && mode !== "legacy" && mode !== null) {
    throw new Error(`_setStorageRouteMode: invalid mode ${mode}`);
  }
  if (mode === null) {
    storageRouteMode = null;
    try { sessionStorage.removeItem(ROUTE_MODE_KEY); } catch (_) {}
  } else {
    setRouteMode(mode);
  }
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
//
// /api/session is upstream's "who am I" endpoint. We MUST await the
// bearer exchange (patch 0001) before calling it — without
// Authorization: Bearer, the runtime's auth_middleware 401s before
// the handler runs and we never learn the user's DID. Used at boot
// by the runtime-identity adoption probe in main.jsx.

export const session = {
  current: async () => {
    await bearerReady.catch(() => false);
    const r = await fetch(apiUrl("/api/session"), {
      credentials: "include",
      headers: authHeaders(),
    });
    return r.ok ? r.json() : null;
  },
};

// Upstream's "has anyone signed up on this node?" endpoint. Returns
// a JSON object (shape varies by upstream version — often has
// has_principal/credential_count/etc.) or null on failure. Useful
// for the adoption probe to distinguish "no user exists" (show
// signup) from "user exists but we can't read their DID" (show
// signin or a more helpful empty state).
export const passkeyStatus = async () => {
  await bearerReady.catch(() => false);
  try {
    const r = await fetch(apiUrl("/api/auth/passkey/status"), {
      credentials: "include",
      headers: authHeaders(),
    });
    return r.ok ? r.json() : null;
  } catch (_) {
    return null;
  }
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
