import axios from "axios";
import {
  generateAuthKey,
  hashAuthKey,
  expandKeypair,
} from "../lib/identity";
import { isCapsuleMode } from "../lib/mode";
import { storage as runtimeStorage, peer, ipfs } from "../lib/runtime";
import { setSession, clearSession, getKeypair, getDidKey } from "../lib/session";
import { createSignedEvent, verifySignedEvent } from "../lib/events";
import { readSharedIdentity, writeSharedIdentity } from "../lib/shell";

const API = axios.create({
  baseURL: "/api",
});

const authHeaders = (token) => ({ Authorization: `Bearer ${token}` });

// Auto-refresh on 401: when an authed request fails with 401, try once to
// swap in a fresh access token via the refresh endpoint, then retry the
// original request. If refresh fails, drop the session and reload to landing.
let refreshing = null;
API.interceptors.response.use(
  (r) => r,
  async (error) => {
    // Defense in depth: in capsule mode there is no Hey backend, no
    // JWT tokens, and no /api/users/refresh route. Every public auth
    // call already short-circuits to the capsule branch BEFORE hitting
    // axios, so this interceptor cannot fire — but if some future code
    // path slips through, we don't want it falling back to a backend
    // URL. Reject immediately.
    if (isCapsuleMode()) return Promise.reject(error);
    const original = error.config || {};
    const status = error.response?.status;
    if (
      status !== 401 ||
      original._retried ||
      original.url?.includes("/users/refresh") ||
      original.url?.includes("/users/signin") ||
      original.url?.includes("/users/signup")
    ) {
      return Promise.reject(error);
    }
    const stored = JSON.parse(localStorage.getItem("profile") || "null");
    if (!stored?.refreshToken) return Promise.reject(error);
    try {
      if (!refreshing) {
        refreshing = axios.post("/api/users/refresh", {
          refreshToken: stored.refreshToken,
        });
      }
      const { data } = await refreshing;
      refreshing = null;
      const next = {
        ...stored,
        accessToken: data.accessToken,
        refreshToken: data.refreshToken,
        user: data.user || stored.user,
      };
      localStorage.setItem("profile", JSON.stringify(next));
      original._retried = true;
      original.headers = {
        ...(original.headers || {}),
        Authorization: `Bearer ${data.accessToken}`,
      };
      return API.request(original);
    } catch (e) {
      refreshing = null;
      localStorage.removeItem("profile");
      if (typeof window !== "undefined") window.location.assign("/");
      return Promise.reject(e);
    }
  }
);

// ─── signup/signin: capsule mode vs server mode ──────────────────────
//
// In server mode (default): POST to Hey's Express backend, which generates
// the authKey + JWT, stores authKeyHash + user record, returns everything.
//
// In capsule mode: there is no Hey backend. We generate the authKey in the
// browser, derive an Ed25519 keypair, compute did:key, hash the authKey for
// later verification, and write the profile to /api/localhost storage via
// the runtime. The shape we return matches the server response so the rest
// of the UI doesn't need to change.

const PROFILE_FILE = "profile.json";

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

const capsuleSignUp = async ({ name }) => {
  if (!name || !name.trim()) {
    throw new Error("Display name is required");
  }

  // 1. If the desktop shell (hey-home) already minted an identity for
  //    this node, adopt it instead of creating a second one. The user
  //    will be prompted to enter their recovery key on the sign-in
  //    screen the first time they need to sign something.
  const shared = await readSharedIdentity();
  if (shared && shared.didKey && shared.recoveryKeyHash) {
    const existingLocal = await runtimeStorage.readJson(PROFILE_FILE);
    if (!existingLocal) {
      const user = newUserRecord({
        name: shared.name || name.trim(),
        didKey: shared.didKey,
        authKeyHash: shared.recoveryKeyHash,
      });
      user.createdAt = shared.createdAt || user.createdAt;
      await runtimeStorage.writeJson(PROFILE_FILE, user);
    }
    const err = new Error(
      "Welcome back — this node already has your identity. Sign in with your recovery key."
    );
    err.code = "ADOPT_SHARED";
    err.response = { data: { message: err.message } };
    throw err;
  }

  // 2. Local profile (Hey's own) already exists — sign in path.
  const existing = await runtimeStorage.readJson(PROFILE_FILE);
  if (existing) {
    const err = new Error("A profile already exists on this node — sign in instead.");
    err.response = { data: { message: err.message } };
    throw err;
  }

  // 3. True first-run signup — mint a fresh identity AND publish it to
  //    the shared identity path so hey-home picks it up on next boot.
  const authKey = generateAuthKey();
  const { didKey } = expandKeypair(authKey);
  const authKeyHash = await hashAuthKey(authKey);

  const user = newUserRecord({ name, didKey, authKeyHash });
  await runtimeStorage.writeJson(PROFILE_FILE, user);

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

const capsuleSignIn = async ({ authKey }) => {
  if (!authKey || !authKey.trim()) {
    throw new Error("Hey key is required");
  }
  const trimmed = authKey.trim();
  const user = await runtimeStorage.readJson(PROFILE_FILE);
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
    await runtimeStorage.writeJson(PROFILE_FILE, user);
  }
  await setSession(trimmed);
  return {
    message: "Signed in successfully",
    user: publicUserShape(user),
    accessToken: "capsule-session",
    refreshToken: "capsule-session",
    accessTokenUpdatedAt: new Date().toISOString(),
  };
};

export const signUp = async (payload) => {
  if (isCapsuleMode()) return capsuleSignUp(payload);
  const response = await API.post("/users/signup", payload, {
    headers: { "Content-Type": "application/json" },
  });
  return response.data;
};

export const signIn = async (payload) => {
  if (isCapsuleMode()) return capsuleSignIn(payload);
  const response = await API.post("/users/signin", payload);
  return response.data;
};

// ─── Migration 2: profile read/update/delete via /api/localhost ──────

const capsuleDeleteAccount = async () => {
  await runtimeStorage.remove(PROFILE_FILE);
  await clearSession();
  return { message: "Account deleted" };
};

const capsuleUpdateProfile = async ({ name, bio, avatar }) => {
  const user = await runtimeStorage.readJson(PROFILE_FILE);
  if (!user) throw new Error("No profile to update");

  if (typeof name === "string") user.name = name.trim().slice(0, 30);
  if (typeof bio === "string") user.bio = bio.trim().slice(0, 280);

  // Avatar is a File/Blob in server mode (multipart). In capsule mode we
  // store it on IPFS, save the CID on the user record. The browser fetches
  // it via the runtime's content gateway.
  if (avatar) {
    const { ipfs, transcoder } = await import("../lib/runtime");
    // Avatars get aggressively shrunk + EXIF-stripped before IPFS.
    const { blob: optimized } = await transcoder.processForUpload(avatar, {
      maxDim: 256, quality: 90, targetFormat: "webp", stripMetadata: true,
    });
    const resp = await ipfs.addBytes(optimized, avatar.name || "avatar", true);
    const cid = resp?.data?.cid || resp?.cid;
    if (!cid) throw new Error("IPFS add_bytes returned no CID");
    user.avatar = `elastos://${cid}`;
    user.avatarCid = cid;
  }

  await runtimeStorage.writeJson(PROFILE_FILE, user);
  return { user: publicUserShape(user) };
};

// Resolve a peer profile by id (which in capsule mode is their did:key).
// Phase 3 will publish profiles via gossip discovery; for now we read from
// the local known-peers cache file, falling back to a stub.
const capsuleGetUserById = async (id) => {
  if (typeof id === "string" && id.startsWith("did:key:z")) {
    const cache = (await runtimeStorage.readJson("peers.json")) || {};
    const entry = cache[id];
    if (entry) return entry;
    return {
      user: { id, name: `${id.slice(0, 16)}…`, didKey: id, avatar: "", bio: "" },
      relationship: "none",
    };
  }
  // If caller passed an old-style server user id, fall back to local profile
  const me = await runtimeStorage.readJson(PROFILE_FILE);
  if (me && me.id === id) {
    return { user: publicUserShape(me), relationship: "self" };
  }
  return null;
};

export const deleteAccount = async (token) => {
  if (isCapsuleMode()) return capsuleDeleteAccount();
  const response = await API.delete("/users/me", { headers: authHeaders(token) });
  return response.data;
};

export const updateProfile = async ({ name, bio, avatar }, token) => {
  if (isCapsuleMode()) return capsuleUpdateProfile({ name, bio, avatar });
  const formData = new FormData();
  if (typeof name === "string") formData.append("name", name);
  if (typeof bio === "string") formData.append("bio", bio);
  if (avatar) formData.append("avatar", avatar);

  const response = await API.patch("/users/me", formData, {
    headers: authHeaders(token),
  });
  return response.data;
};

export const getUserById = async (id, token) => {
  if (isCapsuleMode()) return capsuleGetUserById(id);
  const response = await API.get(`/users/${id}`, token ? { headers: authHeaders(token) } : undefined);
  return response.data;
};

// ─── Capsule-mode helpers for follow, posts, notifications ──────────
//
// Topics:
//   hey-v0/user/<did>/posts    — a user's outgoing post events
//   hey-v0/user/<did>/notif    — notifications inbox (likes, replies, follow requests)
//   hey-v0/follow/<did>        — follow-request / accept / reject events
//
// Local storage:
//   posts/by-id/<id>.json      — own + cached received posts
//   posts/feed.json            — chronological index of post ids (newest first)
//   posts/by-user/<did>.json   — list of post ids by author
//   follows.json               — { following: [did], followers: [did], pending: [did] }
//   notifications/<id>.json    — own notification records
//   notifications/index.json   — sorted index { id → ts, read }

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

const ensureProfile = async () => {
  const me = await runtimeStorage.readJson(PROFILE_FILE);
  if (!me) throw new Error("Not signed in");
  return me;
};

// ─── Follow flow ──────────────────────────────────────────────────

const followsFile = "follows.json";
const readFollows = async () =>
  (await runtimeStorage.readJson(followsFile)) || {
    following: [],
    followers: [],
    pending: [],
  };
const writeFollows = (f) => runtimeStorage.writeJson(followsFile, f);

const capsuleFollowUser = async (peerDid) => {
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

const capsuleUnfollowUser = async (peerDid) => {
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

const capsuleAcceptFollow = async (peerDid) => {
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

const capsuleRejectFollow = async (peerDid) => {
  const follows = await readFollows();
  follows.pending = follows.pending.filter((d) => d !== peerDid);
  await writeFollows(follows);
  await signEventAndPublish(`hey-v0/follow/${peerDid}`, "follow.reject", {
    target_did: peerDid,
    ts: now(),
  });
  return { ok: true };
};

const now = () => Date.now();

// ─── Posts: compose → IPFS for media → signed gossip event ──────────

const readPost = (id) => runtimeStorage.readJson(`posts/by-id/${id}.json`);
const writePost = (id, post) =>
  runtimeStorage.writeJson(`posts/by-id/${id}.json`, post);
const readFeedIndex = async () =>
  (await runtimeStorage.readJson("posts/feed.json")) || [];
const writeFeedIndex = (idx) => runtimeStorage.writeJson("posts/feed.json", idx);

// File → IPFS CID, with type detected from MIME. Transcoder normalizes
// images to WebP @ 2048px and videos to H.264 @ 1080p / CRF 23 before
// the IPFS add, so feed bytes are uniform across whatever cameras
// uploaded them. Falls through to the raw file if hey-transcoder is
// unavailable.
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

const capsuleCreatePost = async ({ caption, images }, _token, onProgress) => {
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

  // Publish + cache locally
  await signEventAndPublish(`hey-v0/user/${me.didKey}/posts`, "post.create", post);
  await writePost(id, post);
  const idx = await readFeedIndex();
  idx.unshift({ id, ts, author: me.didKey });
  await writeFeedIndex(idx);
  return { post };
};

const capsuleGetPosts = async () => {
  const idx = await readFeedIndex();
  const posts = await Promise.all(idx.slice(0, 50).map((e) => readPost(e.id)));
  return { posts: posts.filter(Boolean) };
};

const capsuleGetPost = async (id) => {
  const post = await readPost(id);
  if (!post) throw new Error("Post not found");
  return { post };
};

const capsuleGetUserPosts = async (idOrDid) => {
  const me = await runtimeStorage.readJson(PROFILE_FILE);
  let did = idOrDid;
  if (idOrDid?.startsWith?.("did:key:z")) did = idOrDid;
  else if (me?.id === idOrDid) did = me.didKey;
  const all = await capsuleGetPosts();
  return { posts: all.posts.filter((p) => p.userDid === did) };
};

const capsuleReactToPost = async (postId, emoji) => {
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

const capsuleRepostPost = async (postId) => {
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

const capsuleAddComment = async (postId, text, parentId = null) => {
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

const capsuleReactToComment = async (postId, commentId, emoji) => {
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

const capsuleDeleteComment = async (postId, commentId) => {
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

const capsuleDeletePost = async (postId) => {
  const me = await ensureProfile();
  const post = await readPost(postId);
  if (!post) throw new Error("Post not found");
  if (post.userDid !== me.didKey) throw new Error("Not your post");
  await runtimeStorage.remove(`posts/by-id/${postId}.json`);
  const idx = await readFeedIndex();
  await writeFeedIndex(idx.filter((e) => e.id !== postId));
  await signEventAndPublish(`hey-v0/user/${me.didKey}/posts`, "post.delete", {
    post_id: postId,
    ts: now(),
  });
  return { ok: true };
};

// ─── Notifications ────────────────────────────────────────────────

const capsuleListNotifications = async () =>
  (await runtimeStorage.readJson("notifications/index.json")) || { notifications: [] };

const capsuleMarkNotificationsRead = async () => {
  const wrap = (await runtimeStorage.readJson("notifications/index.json")) || { notifications: [] };
  wrap.notifications = (wrap.notifications || []).map((n) => ({ ...n, read: true }));
  await runtimeStorage.writeJson("notifications/index.json", wrap);
  return wrap;
};

const capsuleDeleteNotification = async (id) => {
  const wrap = (await runtimeStorage.readJson("notifications/index.json")) || { notifications: [] };
  wrap.notifications = (wrap.notifications || []).filter((n) => n.id !== id);
  await runtimeStorage.writeJson("notifications/index.json", wrap);
  return wrap;
};

// ────────────────────────────────────────────────────────────────────
// Public exports — branched
// ────────────────────────────────────────────────────────────────────

export const followUser = async (id, token) => {
  if (isCapsuleMode()) return capsuleFollowUser(id);
  const response = await API.post(`/users/${id}/follow`, {}, { headers: authHeaders(token) });
  return response.data;
};

export const unfollowUser = async (id, token) => {
  if (isCapsuleMode()) return capsuleUnfollowUser(id);
  const response = await API.delete(`/users/${id}/follow`, { headers: authHeaders(token) });
  return response.data;
};

export const acceptFollow = async (id, token) => {
  if (isCapsuleMode()) return capsuleAcceptFollow(id);
  const response = await API.post(`/users/${id}/follow/accept`, {}, { headers: authHeaders(token) });
  return response.data;
};

export const rejectFollow = async (id, token) => {
  if (isCapsuleMode()) return capsuleRejectFollow(id);
  const response = await API.post(`/users/${id}/follow/reject`, {}, { headers: authHeaders(token) });
  return response.data;
};

export const getUserPosts = async (id, token) => {
  if (isCapsuleMode()) return capsuleGetUserPosts(id);
  const response = await API.get(`/posts/by-user/${id}`, token ? { headers: authHeaders(token) } : undefined);
  return response.data;
};

export const listNotifications = async (token) => {
  if (isCapsuleMode()) return capsuleListNotifications();
  const response = await API.get("/notifications", { headers: authHeaders(token) });
  return response.data;
};

export const markNotificationsRead = async (token) => {
  if (isCapsuleMode()) return capsuleMarkNotificationsRead();
  const response = await API.post("/notifications/read-all", {}, { headers: authHeaders(token) });
  return response.data;
};

export const deleteNotification = async (id, token) => {
  if (isCapsuleMode()) return capsuleDeleteNotification(id);
  const response = await API.delete(`/notifications/${id}`, { headers: authHeaders(token) });
  return response.data;
};

export const createPost = async ({ caption, images }, token, onProgress) => {
  if (isCapsuleMode()) return capsuleCreatePost({ caption, images }, token, onProgress);
  const formData = new FormData();
  formData.append("caption", caption || "");
  for (const file of images || []) {
    formData.append("media", file);
  }

  const response = await API.post("/posts", formData, {
    headers: authHeaders(token),
    onUploadProgress: (event) => {
      if (event.total && onProgress) {
        onProgress(Math.round((event.loaded / event.total) * 100));
      }
    },
  });
  return response.data;
};

export const getPosts = async (token) => {
  if (isCapsuleMode()) return capsuleGetPosts();
  const response = await API.get(
    "/posts",
    token ? { headers: authHeaders(token) } : undefined
  );
  return response.data;
};

export const getPost = async (id, token) => {
  if (isCapsuleMode()) return capsuleGetPost(id);
  const response = await API.get(
    `/posts/${id}`,
    token ? { headers: authHeaders(token) } : undefined
  );
  return response.data;
};

export const reactToPost = async (id, emoji, token) => {
  if (isCapsuleMode()) return capsuleReactToPost(id, emoji);
  const response = await API.post(
    `/posts/${id}/react`,
    { emoji },
    { headers: authHeaders(token) }
  );
  return response.data;
};

export const repostPost = async (id, token) => {
  if (isCapsuleMode()) return capsuleRepostPost(id);
  const response = await API.post(
    `/posts/${id}/repost`,
    {},
    { headers: authHeaders(token) }
  );
  return response.data;
};

export const addComment = async (id, text, token, parentId = null) => {
  if (isCapsuleMode()) return capsuleAddComment(id, text, parentId);
  const response = await API.post(
    `/posts/${id}/comments`,
    parentId ? { text, parentId } : { text },
    { headers: authHeaders(token) }
  );
  return response.data;
};

export const reactToComment = async (postId, commentId, emoji, token) => {
  if (isCapsuleMode()) return capsuleReactToComment(postId, commentId, emoji);
  const response = await API.post(
    `/posts/${postId}/comments/${commentId}/react`,
    { emoji },
    { headers: authHeaders(token) }
  );
  return response.data;
};

export const deleteComment = async (postId, commentId, token) => {
  if (isCapsuleMode()) return capsuleDeleteComment(postId, commentId);
  const response = await API.delete(
    `/posts/${postId}/comments/${commentId}`,
    { headers: authHeaders(token) }
  );
  return response.data;
};

export const deletePost = async (id, token) => {
  if (isCapsuleMode()) return capsuleDeletePost(id);
  const response = await API.delete(`/posts/${id}`, { headers: authHeaders(token) });
  return response.data;
};
