// Runtime API client — Hey's browser-side wrapper around the Elastos Runtime's
// HTTP surface. Replaces axios calls to Hey's Express backend when the SPA
// is running inside a WASM capsule.
//
// Two endpoints matter most:
//
//   POST /api/provider/:scheme/:op  — capability-gated proxy to any provider.
//     Carrier ops live under :scheme=peer (gossip_send/recv/join/leave/etc.),
//     IPFS under :scheme=ipfs (add_bytes/cat/pin/etc.),
//     DID resolve+sign+verify under :scheme=did.
//
//   GET/PUT/DELETE /api/localhost/*path  — sandboxed storage CRUD.
//     Hey's data dir under localhost://Users/self/.AppData/LocalHost/Hey/*.
//     Stores profile.json, chat messages, notif state, etc.
//
// Auth: capability tokens via X-Capability-Token header. Shell sessions
// (running inside Home's launched iframe) get a token automatically; outside
// that we'd need /api/capability/request → grant flow. For now, prefer the
// shell-session path.

const STORAGE_BASE = "/api/localhost/Users/self/.AppData/LocalHost/Hey";
const PROVIDER_BASE = "/api/provider";

// Cached capability tokens keyed by scheme. Acquired lazily on first use.
const tokenCache = new Map();

// In-memory session token if the runtime sets one in a cookie / response.
// Some runtime configurations expose tokens via /api/orchestrator/session.
let sessionToken = null;

export const setSessionToken = (token) => {
  sessionToken = token;
};

const authHeaders = () => {
  const h = {};
  if (sessionToken) h["X-Capability-Token"] = sessionToken;
  return h;
};

// ─── Provider calls ────────────────────────────────────────────────

// Generic provider-proxy call: POST /api/provider/<scheme>/<op> with JSON body.
// Returns the parsed JSON response. Throws on HTTP error.
export const providerCall = async (scheme, op, body = {}) => {
  const resp = await fetch(`${PROVIDER_BASE}/${encodeURIComponent(scheme)}/${encodeURIComponent(op)}`, {
    method: "POST",
    credentials: "include",
    headers: {
      "Content-Type": "application/json",
      ...authHeaders(),
    },
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

  // Construct a URL the runtime gateway serves directly. Useful for <img src>.
  // We use the namespace endpoint which the runtime resolves to a content
  // stream without a base64 round-trip.
  gatewayUrl: (cid, path) => {
    const suffix = path ? `/${path.replace(/^\/+/, "")}` : "";
    return `/api/localhost/WebSpaces/Elastos/content/${encodeURIComponent(cid)}${suffix}`;
  },

  pin: (cid) => providerCall("ipfs", "pin", { cid }),
  unpin: (cid) => providerCall("ipfs", "unpin", { cid }),
  ls: (cid) => providerCall("ipfs", "ls", { cid }),
  health: () => providerCall("ipfs", "health", {}),
};

// ─── DID provider (resolve / verify any did:key) ───────────────────

export const did = {
  // Resolve any did:key — returns pubkey + DID doc. Works for both runtime-
  // issued machine DIDs and Hey-issued user DIDs (same format).
  resolve: (did) => providerCall("did", "resolve", { did }),
};

// ─── Localhost storage CRUD ────────────────────────────────────────

const storagePath = (relative) =>
  `${STORAGE_BASE}/${(relative || "").replace(/^\/+/, "")}`;

export const storage = {
  // Read a path. Returns parsed JSON, raw text, or Uint8Array (caller hints).
  readJson: async (path) => {
    const resp = await fetch(storagePath(path), {
      credentials: "include",
      headers: authHeaders(),
    });
    if (resp.status === 404) return null;
    if (!resp.ok)
      throw new RuntimeError(`localhost GET ${path}: HTTP ${resp.status}`);
    return resp.json();
  },

  writeJson: async (path, value) => {
    const resp = await fetch(storagePath(path), {
      method: "PUT",
      credentials: "include",
      headers: { "Content-Type": "application/json", ...authHeaders() },
      body: JSON.stringify(value),
    });
    if (!resp.ok) {
      const txt = await resp.text().catch(() => "");
      throw new RuntimeError(`localhost PUT ${path}: HTTP ${resp.status}`, {
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
      headers: authHeaders(),
    });
    if (!resp.ok && resp.status !== 404)
      throw new RuntimeError(`localhost DELETE ${path}: HTTP ${resp.status}`);
    return true;
  },

  list: async (path) => {
    const resp = await fetch(`${storagePath(path)}?list=true`, {
      credentials: "include",
      headers: authHeaders(),
    });
    if (resp.status === 404) return [];
    if (!resp.ok)
      throw new RuntimeError(`localhost LIST ${path}: HTTP ${resp.status}`);
    return resp.json();
  },

  mkdir: async (path) => {
    const resp = await fetch(`${storagePath(path)}?mkdir=true`, {
      method: "POST",
      credentials: "include",
      headers: authHeaders(),
    });
    if (!resp.ok && resp.status !== 409)
      throw new RuntimeError(`localhost MKDIR ${path}: HTTP ${resp.status}`);
    return true;
  },
};

// ─── Capability flow (operator-grant model) ────────────────────────

export const capability = {
  request: ({ resource, action }) =>
    fetch("/api/capability/request", {
      method: "POST",
      credentials: "include",
      headers: { "Content-Type": "application/json", ...authHeaders() },
      body: JSON.stringify({ resource, action }),
    }).then((r) => r.json()),
  status: (id) =>
    fetch(`/api/capability/request/${encodeURIComponent(id)}`, {
      credentials: "include",
      headers: authHeaders(),
    }).then((r) => r.json()),
  list: () =>
    fetch("/api/capability/list", {
      credentials: "include",
      headers: authHeaders(),
    }).then((r) => r.json()),
};

// ─── Session helpers ───────────────────────────────────────────────

export const session = {
  current: () =>
    fetch("/api/session", {
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
