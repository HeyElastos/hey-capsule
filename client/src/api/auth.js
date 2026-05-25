import axios from "axios";
import {
  generateAuthKey,
  hashAuthKey,
  expandKeypair,
} from "../lib/identity";
import { isCapsuleMode } from "../lib/mode";
import { storage as runtimeStorage } from "../lib/runtime";

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
  const existing = await runtimeStorage.readJson(PROFILE_FILE);
  if (existing) {
    const err = new Error("A profile already exists on this node — sign in instead.");
    err.response = { data: { message: err.message } };
    throw err;
  }
  const authKey = generateAuthKey();
  const { didKey } = expandKeypair(authKey);
  const authKeyHash = await hashAuthKey(authKey);

  const user = newUserRecord({ name, didKey, authKeyHash });
  await runtimeStorage.writeJson(PROFILE_FILE, user);

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

export const deleteAccount = async (token) => {
  const response = await API.delete("/users/me", { headers: authHeaders(token) });
  return response.data;
};

export const updateProfile = async ({ name, bio, avatar }, token) => {
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
  const response = await API.get(`/users/${id}`, token ? { headers: authHeaders(token) } : undefined);
  return response.data;
};

export const followUser = async (id, token) => {
  const response = await API.post(`/users/${id}/follow`, {}, { headers: authHeaders(token) });
  return response.data;
};

export const unfollowUser = async (id, token) => {
  const response = await API.delete(`/users/${id}/follow`, { headers: authHeaders(token) });
  return response.data;
};

export const acceptFollow = async (id, token) => {
  const response = await API.post(`/users/${id}/follow/accept`, {}, { headers: authHeaders(token) });
  return response.data;
};

export const rejectFollow = async (id, token) => {
  const response = await API.post(`/users/${id}/follow/reject`, {}, { headers: authHeaders(token) });
  return response.data;
};

export const getUserPosts = async (id, token) => {
  const response = await API.get(`/posts/by-user/${id}`, token ? { headers: authHeaders(token) } : undefined);
  return response.data;
};

export const listNotifications = async (token) => {
  const response = await API.get("/notifications", { headers: authHeaders(token) });
  return response.data;
};

export const markNotificationsRead = async (token) => {
  const response = await API.post("/notifications/read-all", {}, { headers: authHeaders(token) });
  return response.data;
};

export const deleteNotification = async (id, token) => {
  const response = await API.delete(`/notifications/${id}`, { headers: authHeaders(token) });
  return response.data;
};

export const createPost = async ({ caption, images }, token, onProgress) => {
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
  const response = await API.get(
    "/posts",
    token ? { headers: authHeaders(token) } : undefined
  );
  return response.data;
};

export const getPost = async (id, token) => {
  const response = await API.get(
    `/posts/${id}`,
    token ? { headers: authHeaders(token) } : undefined
  );
  return response.data;
};

export const reactToPost = async (id, emoji, token) => {
  const response = await API.post(
    `/posts/${id}/react`,
    { emoji },
    { headers: authHeaders(token) }
  );
  return response.data;
};

export const repostPost = async (id, token) => {
  const response = await API.post(
    `/posts/${id}/repost`,
    {},
    { headers: authHeaders(token) }
  );
  return response.data;
};

export const addComment = async (id, text, token, parentId = null) => {
  const response = await API.post(
    `/posts/${id}/comments`,
    parentId ? { text, parentId } : { text },
    { headers: authHeaders(token) }
  );
  return response.data;
};

export const reactToComment = async (postId, commentId, emoji, token) => {
  const response = await API.post(
    `/posts/${postId}/comments/${commentId}/react`,
    { emoji },
    { headers: authHeaders(token) }
  );
  return response.data;
};

export const deleteComment = async (postId, commentId, token) => {
  const response = await API.delete(
    `/posts/${postId}/comments/${commentId}`,
    { headers: authHeaders(token) }
  );
  return response.data;
};

export const deletePost = async (id, token) => {
  const response = await API.delete(`/posts/${id}`, { headers: authHeaders(token) });
  return response.data;
};
