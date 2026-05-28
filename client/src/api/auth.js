// Hey Social — capsule-only API layer.
//
// All data flows through the Elastos Runtime:
//   - storage:  /api/apps/hey-social/storage/Hey/* (profile, follows,
//               post cache, notification index — per-capsule namespace
//               under the per-user principal root)
//   - peer:     /api/provider/peer/* (Carrier gossip — posts, follows, comments)
//   - ipfs:     /api/provider/ipfs/* (post media + avatar storage)
//   - did:      /api/provider/did/* (DID resolution for unknown senders)
//   - shell.js: /api/apps/hey-social/storage/.AppData/Identity/profile.json
//               (shared identity with the host shell, e.g. hey-home —
//               shares the principal root, sits OUTSIDE the Hey/ prefix)
//
// There is no Hey-owned backend. Signing happens with a non-extractable
// Web Crypto Ed25519 key kept in IndexedDB (see lib/keystore.js).

import {
  generateAuthKey,
  hashAuthKey,
  expandKeypair,
} from "../lib/identity";
import { storage, peer, ipfs } from "../lib/runtime";
import { setSession, clearSession, getKeypair } from "../lib/session";
import { deleteSigningKey } from "../lib/keystore";
import { createSignedEvent } from "../lib/events";
import { readSharedIdentity, writeSharedIdentity, deleteSharedIdentity } from "../lib/shell";
import * as heyVault from "../lib/vault";

const PROFILE_FILE = "profile.json";
const FOLLOWS_FILE = "follows.json";

const now = () => Date.now();

const newUserRecord = ({ name, didKey, authKeyHash }) => ({
  id: crypto.randomUUID(),
  name: name.trim().slice(0, 30),
  authKeyHash,
  didKey,
  role: "general",
  avatar: "",
  bio: "",
  followers: [],
  following: [],
  pendingFollowers: [],
  pendingFollowing: [],
  createdAt: new Date().toISOString(),
});

const publicUserShape = (user) => ({
  id: user.id,
  name: user.name,
  bio: user.bio || "",
  avatar: user.avatar || "",
  role: user.role,
  didKey: user.didKey || "",
  counts: {
    followers: (user.followers || []).length,
    following: (user.following || []).length,
  },
});

const ensureProfile = async () => {
  // Try the Hey-specific profile first (the canonical location for app
  // metadata: avatar, bio, follows). If it's missing — e.g. the user
  // signed in via passkey and never went through the nickname flow that
  // writes it — fall back to the shared cross-app identity at
  // .AppData/Identity/profile.json (written by the home welcome flow
  // and shared with hey-home). Materialize a minimal Hey record from
  // it so subsequent calls don't have to repeat the fallback.
  let me = await storage.readJson(PROFILE_FILE);
  if (me) {
    // SECURITY backfill: pre-fix passkey signups (before db9ae38) never
    // wrote the shared identity, leaving the home shell's lock screen
    // believing the node was uninitialized — a stranger on a new device
    // could complete the signup wizard and overwrite this user's
    // identity. If a Hey profile exists but no shared identity does,
    // publish one now. Idempotent: a one-shot migration that no-ops on
    // subsequent calls.
    try {
      const shared = await readSharedIdentity().catch(() => null);
      if (!shared || !shared.didKey) {
        await writeSharedIdentity({
          name: me.name || "Hey user",
          didKey: me.didKey,
          recoveryKeyHash: me.authKeyHash || "",
          passkeys: [],
          avatar: me.avatar || "",
          bio: me.bio || "",
          createdAt: me.createdAt || new Date().toISOString(),
          createdBy: "hey-backfill",
        });
        console.info("[hey] backfilled shared identity from local profile");
      }
    } catch (err) {
      console.warn("[hey] shared identity backfill failed", err);
    }
    return me;
  }

  const shared = await readSharedIdentity().catch(() => null);
  const kp = getKeypair();
  const didKey = shared?.didKey || kp?.didKey || null;
  if (!didKey) throw new Error("Not signed in");

  me = newUserRecord({
    name: shared?.name || "Hey user",
    didKey,
    authKeyHash: shared?.recoveryKeyHash || "",
  });
  // avatar / bio from shared identity (if shell saved them there)
  if (shared?.avatar) me.avatar = shared.avatar;
  if (shared?.bio) me.bio = shared.bio;
  // Persist so future reads hit the local copy.
  await storage.writeJson(PROFILE_FILE, me).catch(() => {});
  return me;
};

// ── Sign + publish a signed gossip event ─────────────────────────────

const signEventAndPublish = async (topic, type, payload) => {
  const kp = getKeypair();
  if (!kp) throw new Error("Not signed in");
  const event = await createSignedEvent({ type, payload }, kp);
  await peer.publish({
    topic,
    message: JSON.stringify(event),
    sender_id: event.sender_did,
    ts: event.ts,
    signature: event.signature,
  });
  return event;
};

// ─── Signup / sign-in ───────────────────────────────────────────────
//
// Recovery key is generated in the browser, Ed25519 keypair derived
// from it, then the seed is imported as a non-extractable Web Crypto
// key and persisted in IndexedDB. The raw seed never appears in
// storage. SHA-256(seed-hex) is what we save as `authKeyHash` so we
// can verify a re-entered recovery key without keeping the secret.

export const signUp = async ({ name }) => {
  if (!name || !name.trim()) {
    throw new Error("Display name is required");
  }

  // 1. If the desktop shell (e.g. hey-home) already minted an identity
  //    for this node, adopt it instead of creating a second one. The
  //    user will be prompted to enter their recovery key on the sign-in
  //    screen the first time they need to sign something.
  const shared = await readSharedIdentity();
  if (shared && shared.didKey && shared.recoveryKeyHash) {
    const existingLocal = await storage.readJson(PROFILE_FILE);
    if (!existingLocal) {
      const user = newUserRecord({
        name: shared.name || name.trim(),
        didKey: shared.didKey,
        authKeyHash: shared.recoveryKeyHash,
      });
      user.createdAt = shared.createdAt || user.createdAt;
      await storage.writeJson(PROFILE_FILE, user);
    }
    const err = new Error(
      "Welcome back — this node already has your identity. Sign in with your recovery key."
    );
    err.code = "ADOPT_SHARED";
    err.response = { data: { message: err.message } };
    throw err;
  }

  // 2. Local profile already exists — sign in path.
  const existing = await storage.readJson(PROFILE_FILE);
  if (existing) {
    const err = new Error("A profile already exists on this node — sign in instead.");
    err.response = { data: { message: err.message } };
    throw err;
  }

  // 3. True first-run signup — mint a fresh identity AND publish it to
  //    the shared identity path so the desktop shell picks it up on next boot.
  const authKey = generateAuthKey();
  const { didKey } = expandKeypair(authKey);
  const authKeyHash = await hashAuthKey(authKey);

  const user = newUserRecord({ name, didKey, authKeyHash });
  await storage.writeJson(PROFILE_FILE, user);

  // Import the seed as a non-extractable Web Crypto key and persist
  // the handle in IndexedDB. setSession zeroes the seed in place after
  // import — see lib/session.js + lib/keystore.js.
  await setSession(authKey);

  // Publish to shared identity contract so any other capsule (shell,
  // companion apps) on this node uses the same identity.
  await writeSharedIdentity({
    name,
    didKey,
    recoveryKeyHash: authKeyHash,
    passkeys: [],
    createdAt: new Date().toISOString(),
    createdBy: "hey",
  });

  return {
    message: "User created successfully",
    user: publicUserShape(user),
    authKey,
    accessToken: "capsule-session",
    refreshToken: "capsule-session",
    accessTokenUpdatedAt: new Date().toISOString(),
  };
};

export const signIn = async ({ authKey }) => {
  if (!authKey || !authKey.trim()) {
    throw new Error("Hey key is required");
  }
  const trimmed = authKey.trim();
  const user = await storage.readJson(PROFILE_FILE);
  if (!user) {
    const err = new Error("No profile on this node — sign up first.");
    err.response = { data: { message: err.message } };
    throw err;
  }
  const hash = await hashAuthKey(trimmed);
  if (hash !== user.authKeyHash) {
    const err = new Error("Invalid Hey key");
    err.response = { status: 401, data: { message: err.message } };
    throw err;
  }
  if (!user.didKey) {
    user.didKey = expandKeypair(trimmed).didKey;
    await storage.writeJson(PROFILE_FILE, user);
  }
  await setSession(trimmed);
  // If the user has a vault, the recovery key they just entered also
  // unwraps the master key (recovery wrap is always set at signup).
  // Non-fatal on failure: signin still succeeds, vault stays locked.
  if (await heyVault.hasVault()) {
    try {
      await heyVault.unlockVaultWithRecovery(trimmed);
    } catch (err) {
      console.warn("[hey] vault unlock via recovery key failed", err);
    }
  }
  return {
    message: "Signed in successfully",
    user: publicUserShape(user),
    accessToken: "capsule-session",
    refreshToken: "capsule-session",
    accessTokenUpdatedAt: new Date().toISOString(),
  };
};

// ─── Profile read / update / delete ─────────────────────────────────

export const deleteAccount = async () => {
  // Scrub every layer the identity lives on. Best-effort: any single
  // step failing must not block the others, so the user can retry and
  // still get a clean wipe.
  const tryDo = (label, fn) =>
    Promise.resolve()
      .then(fn)
      .catch((err) => console.warn(`[hey] deleteAccount: ${label} failed`, err));

  // 1) Server-side: Hey profile + follows + vault wraps + passkey creds
  //    + shared cross-app identity. Each of these is in localhost storage
  //    under the runtime.
  await tryDo("remove Hey profile",   () => storage.remove(PROFILE_FILE));
  await tryDo("remove follows",       () => storage.remove(FOLLOWS_FILE));
  await tryDo("remove vault wraps",   () => storage.remove("vault-wraps.json"));
  await tryDo("remove passkey creds", () => storage.remove("passkey-creds.json"));
  await tryDo("remove shared identity", deleteSharedIdentity);

  // 2) Client-side: IndexedDB-stored signing key (non-extractable
  //    CryptoKey persists across cookie clears; only Clear All Site
  //    Data or this explicit delete wipes it).
  await tryDo("delete signing key", deleteSigningKey);

  // 3) Vault state cached in module-level memory.
  await tryDo("lock vault", () => heyVault.lockVault?.());

  // 4) Bearer + capability + runtime tokens in sessionStorage.
  try {
    sessionStorage.removeItem("hey-runtime-token");
    sessionStorage.removeItem("hey-capability-tokens");
    sessionStorage.removeItem("hey-capsule-token-cache");
  } catch (_) { /* private mode etc. */ }

  // 5) Local profile state (the in-memory hook + the localStorage cache).
  await tryDo("clear session", clearSession);

  return { message: "Account deleted" };
};

export const updateProfile = async ({ name, bio, avatar }) => {
  const user = await storage.readJson(PROFILE_FILE);
  if (!user) throw new Error("No profile to update");

  if (typeof name === "string") user.name = name.trim().slice(0, 30);
  if (typeof bio === "string") user.bio = bio.trim().slice(0, 280);

  // Avatar bytes → IPFS CID. Aggressively transcoded to a small WebP
  // with EXIF stripped before pinning. The runtime serves the CID via
  // its content gateway when read.
  if (avatar) {
    const { transcoder } = await import("../lib/runtime");
    const { blob: optimized } = await transcoder.processForUpload(avatar, {
      maxDim: 256, quality: 90, targetFormat: "webp", stripMetadata: true,
    });
    const resp = await ipfs.addBytes(optimized, avatar.name || "avatar", true);
    const cid = resp?.data?.cid || resp?.cid;
    if (!cid) throw new Error("IPFS add_bytes returned no CID");
    user.avatar = `elastos://${cid}`;
    user.avatarCid = cid;
  }

  await storage.writeJson(PROFILE_FILE, user);
  return { user: publicUserShape(user) };
};

// Resolve a peer profile by id, which is their did:key. Phase 3 will
// publish profiles via gossip discovery; for now we read from the
// local known-peers cache, falling back to a stub.
export const getUserById = async (id) => {
  if (typeof id === "string" && id.startsWith("did:key:z")) {
    const cache = (await storage.readJson("peers.json")) || {};
    const entry = cache[id];
    if (entry) return entry;
    return {
      user: { id, name: `${id.slice(0, 16)}…`, didKey: id, avatar: "", bio: "" },
      relationship: "none",
    };
  }
  const me = await storage.readJson(PROFILE_FILE);
  if (me && me.id === id) {
    return { user: publicUserShape(me), relationship: "self" };
  }
  return null;
};

// ─── Follow flow ─────────────────────────────────────────────────────
//
// Topics:
//   hey-v0/user/<did>/posts    — a user's outgoing post events
//   hey-v0/user/<did>/notif    — notifications inbox
//   hey-v0/follow/<did>        — follow-request / accept / reject events
//
// Local storage:
//   posts/by-id/<id>.json      — own + cached received posts
//   posts/feed.json            — chronological index of post ids
//   follows.json               — { following: [did], followers: [did], pending: [did] }
//   notifications/index.json   — sorted notifications index

const readFollows = async () =>
  (await storage.readJson(FOLLOWS_FILE)) || {
    following: [],
    followers: [],
    pending: [],
  };
const writeFollows = (f) => storage.writeJson(FOLLOWS_FILE, f);

export const followUser = async (peerDid) => {
  const me = await ensureProfile();
  if (!peerDid?.startsWith?.("did:key:z")) throw new Error("Invalid did");
  if (peerDid === me.didKey) throw new Error("Cannot follow yourself");

  await peer.joinTopic(`hey-v0/user/${peerDid}/posts`);

  const follows = await readFollows();
  if (!follows.following.includes(peerDid)) follows.following.push(peerDid);
  await writeFollows(follows);

  await signEventAndPublish(`hey-v0/follow/${peerDid}`, "follow.request", {
    target_did: peerDid,
    from_name: me.name,
    ts: now(),
  });
  return { ok: true };
};

export const unfollowUser = async (peerDid) => {
  await peer.leaveTopic(`hey-v0/user/${peerDid}/posts`);
  const follows = await readFollows();
  follows.following = follows.following.filter((d) => d !== peerDid);
  await writeFollows(follows);
  await signEventAndPublish(`hey-v0/follow/${peerDid}`, "follow.unfollow", {
    target_did: peerDid,
    ts: now(),
  });
  return { ok: true };
};

export const acceptFollow = async (peerDid) => {
  const follows = await readFollows();
  follows.pending = follows.pending.filter((d) => d !== peerDid);
  if (!follows.followers.includes(peerDid)) follows.followers.push(peerDid);
  await writeFollows(follows);
  await signEventAndPublish(`hey-v0/follow/${peerDid}`, "follow.accept", {
    target_did: peerDid,
    ts: now(),
  });
  return { ok: true };
};

export const rejectFollow = async (peerDid) => {
  const follows = await readFollows();
  follows.pending = follows.pending.filter((d) => d !== peerDid);
  await writeFollows(follows);
  await signEventAndPublish(`hey-v0/follow/${peerDid}`, "follow.reject", {
    target_did: peerDid,
    ts: now(),
  });
  return { ok: true };
};

// ─── Posts: compose → IPFS for media → signed gossip event ──────────

const readPost = (id) => storage.readJson(`posts/by-id/${id}.json`);
const writePost = (id, post) => storage.writeJson(`posts/by-id/${id}.json`, post);
const readFeedIndex = async () =>
  (await storage.readJson("posts/feed.json")) || [];
const writeFeedIndex = (idx) => storage.writeJson("posts/feed.json", idx);

// File → IPFS CID. Transcoder normalizes images to WebP @ 2048px and
// videos to H.264 @ 1080p / CRF 23 before the IPFS add, so feed bytes
// are uniform across whatever cameras uploaded them. Falls through to
// the raw file if hey-transcoder is unavailable.
const ipfsUploadMedia = async (file) => {
  const { transcoder } = await import("../lib/runtime");
  const { blob } = await transcoder.processForUpload(file);
  const resp = await ipfs.addBytes(blob, file.name || "media", true);
  const cid = resp?.data?.cid || resp?.cid;
  if (!cid) throw new Error("IPFS add_bytes returned no CID");
  const isVideo = file.type?.startsWith?.("video/");
  return {
    url: `elastos://${cid}`,
    cid,
    type: isVideo ? "video" : "photo",
    mime: file.type || "",
    name: file.name || "",
  };
};

export const createPost = async ({ caption, images }, onProgress) => {
  const me = await ensureProfile();
  const total = (images || []).length;

  const uploaded = [];
  for (let i = 0; i < total; i++) {
    const tile = await ipfsUploadMedia(images[i]);
    uploaded.push(tile);
    if (onProgress) onProgress(Math.round(((i + 1) / Math.max(total, 1)) * 100));
  }

  const id = crypto.randomUUID();
  const ts = now();
  const post = {
    id,
    userId: me.id,
    userDid: me.didKey,
    userName: me.name,
    userAvatar: me.avatar || "",
    caption: (caption || "").slice(0, 2200),
    images: uploaded,
    createdAt: new Date(ts).toISOString(),
    reactions: {},
    reposts: [],
    comments: [],
    ts,
  };

  await signEventAndPublish(`hey-v0/user/${me.didKey}/posts`, "post.create", post);
  await writePost(id, post);
  const idx = await readFeedIndex();
  idx.unshift({ id, ts, author: me.didKey });
  await writeFeedIndex(idx);
  return { post };
};

export const getPosts = async () => {
  const idx = await readFeedIndex();
  const posts = await Promise.all(idx.slice(0, 50).map((e) => readPost(e.id)));
  return { posts: posts.filter(Boolean) };
};

export const getPost = async (id) => {
  const post = await readPost(id);
  if (!post) throw new Error("Post not found");
  return { post };
};

export const getUserPosts = async (idOrDid) => {
  const me = await storage.readJson(PROFILE_FILE);
  let did = idOrDid;
  if (idOrDid?.startsWith?.("did:key:z")) did = idOrDid;
  else if (me?.id === idOrDid) did = me.didKey;
  const all = await getPosts();
  return { posts: all.posts.filter((p) => p.userDid === did) };
};

export const reactToPost = async (postId, emoji) => {
  const me = await ensureProfile();
  const post = await readPost(postId);
  if (!post) throw new Error("Post not found");
  const reactions = { ...(post.reactions || {}) };
  const list = reactions[emoji] || [];
  const i = list.indexOf(me.didKey);
  if (i >= 0) list.splice(i, 1);
  else list.push(me.didKey);
  if (list.length === 0) delete reactions[emoji];
  else reactions[emoji] = list;
  post.reactions = reactions;
  await writePost(postId, post);
  await signEventAndPublish(`hey-v0/user/${post.userDid}/posts`, "post.react", {
    post_id: postId,
    emoji,
    reactor_did: me.didKey,
    ts: now(),
  });
  return { post };
};

export const repostPost = async (postId) => {
  const me = await ensureProfile();
  const post = await readPost(postId);
  if (!post) throw new Error("Post not found");
  const reposts = Array.from(new Set([...(post.reposts || []), me.didKey]));
  post.reposts = reposts;
  await writePost(postId, post);
  await signEventAndPublish(`hey-v0/user/${post.userDid}/posts`, "post.repost", {
    post_id: postId,
    reposter_did: me.didKey,
    ts: now(),
  });
  return { post };
};

export const addComment = async (postId, text, parentId = null) => {
  const me = await ensureProfile();
  const post = await readPost(postId);
  if (!post) throw new Error("Post not found");
  const comment = {
    id: crypto.randomUUID(),
    userId: me.id,
    userDid: me.didKey,
    userName: me.name,
    text: (text || "").slice(0, 500),
    parentId,
    createdAt: new Date().toISOString(),
    reactions: {},
    ts: now(),
  };
  post.comments = [...(post.comments || []), comment];
  await writePost(postId, post);
  await signEventAndPublish(`hey-v0/user/${post.userDid}/posts`, "post.comment", {
    post_id: postId,
    comment,
  });
  return { post, comment };
};

export const reactToComment = async (postId, commentId, emoji) => {
  const me = await ensureProfile();
  const post = await readPost(postId);
  if (!post) throw new Error("Post not found");
  const comment = (post.comments || []).find((c) => c.id === commentId);
  if (!comment) throw new Error("Comment not found");
  const reactions = { ...(comment.reactions || {}) };
  const list = reactions[emoji] || [];
  const i = list.indexOf(me.didKey);
  if (i >= 0) list.splice(i, 1);
  else list.push(me.didKey);
  if (list.length === 0) delete reactions[emoji];
  else reactions[emoji] = list;
  comment.reactions = reactions;
  await writePost(postId, post);
  await signEventAndPublish(`hey-v0/user/${post.userDid}/posts`, "post.comment_react", {
    post_id: postId,
    comment_id: commentId,
    emoji,
    reactor_did: me.didKey,
    ts: now(),
  });
  return { post };
};

export const deleteComment = async (postId, commentId) => {
  const me = await ensureProfile();
  const post = await readPost(postId);
  if (!post) throw new Error("Post not found");
  post.comments = (post.comments || []).filter((c) => c.id !== commentId);
  await writePost(postId, post);
  await signEventAndPublish(`hey-v0/user/${post.userDid}/posts`, "post.comment_delete", {
    post_id: postId,
    comment_id: commentId,
    deleter_did: me.didKey,
    ts: now(),
  });
  return { post };
};

export const deletePost = async (postId) => {
  const me = await ensureProfile();
  const post = await readPost(postId);
  if (!post) throw new Error("Post not found");
  if (post.userDid !== me.didKey) throw new Error("Not your post");
  await storage.remove(`posts/by-id/${postId}.json`);
  const idx = await readFeedIndex();
  await writeFeedIndex(idx.filter((e) => e.id !== postId));
  await signEventAndPublish(`hey-v0/user/${me.didKey}/posts`, "post.delete", {
    post_id: postId,
    ts: now(),
  });
  return { ok: true };
};

// ─── Notifications ───────────────────────────────────────────────────

export const listNotifications = async () =>
  (await storage.readJson("notifications/index.json")) || { notifications: [] };

export const markNotificationsRead = async () => {
  const wrap = (await storage.readJson("notifications/index.json")) || { notifications: [] };
  wrap.notifications = (wrap.notifications || []).map((n) => ({ ...n, read: true }));
  await storage.writeJson("notifications/index.json", wrap);
  return wrap;
};

export const deleteNotification = async (id) => {
  const wrap = (await storage.readJson("notifications/index.json")) || { notifications: [] };
  wrap.notifications = (wrap.notifications || []).filter((n) => n.id !== id);
  await storage.writeJson("notifications/index.json", wrap);
  return wrap;
};
