import axios from "axios";

const API = axios.create({
  baseURL: "/api",
});

const authHeaders = (token) => ({ Authorization: `Bearer ${token}` });

export const signUp = async (payload) => {
  const response = await API.post("/users/signup", payload, {
    headers: { "Content-Type": "application/json" },
  });
  return response.data;
};

export const signIn = async (payload) => {
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

export const addComment = async (id, text, token) => {
  const response = await API.post(
    `/posts/${id}/comments`,
    { text },
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
