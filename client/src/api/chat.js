import axios from "axios";

const API = axios.create({ baseURL: "/api" });
const auth = (token) => ({ Authorization: `Bearer ${token}` });

export const listThreads = async (token) => {
  const { data } = await API.get("/chat/threads", { headers: auth(token) });
  return data.threads || [];
};

export const getThread = async (token, peerDid, opts = {}) => {
  const params = new URLSearchParams();
  if (opts.before) params.set("before", String(opts.before));
  if (opts.limit) params.set("limit", String(opts.limit));
  const qs = params.toString() ? `?${params}` : "";
  const { data } = await API.get(`/chat/threads/${encodeURIComponent(peerDid)}${qs}`, {
    headers: auth(token),
  });
  return data;
};

export const sendMessage = async (token, peerDid, content, replyTo = null) => {
  const body = { content };
  if (replyTo) body.reply_to = replyTo;
  const { data } = await API.post(
    `/chat/threads/${encodeURIComponent(peerDid)}/messages`,
    body,
    { headers: auth(token) }
  );
  return data.message;
};

export const editMessage = async (token, messageId, content) => {
  const { data } = await API.patch(
    `/chat/messages/${encodeURIComponent(messageId)}`,
    { content },
    { headers: auth(token) }
  );
  return data.message;
};

export const deleteMessage = async (token, messageId) => {
  const { data } = await API.delete(`/chat/messages/${encodeURIComponent(messageId)}`, {
    headers: auth(token),
  });
  return data.message;
};

export const reactToMessage = async (token, messageId, emoji) => {
  const { data } = await API.post(
    `/chat/messages/${encodeURIComponent(messageId)}/reactions`,
    { emoji },
    { headers: auth(token) }
  );
  return data.message;
};

export const markThreadRead = async (token, peerDid) => {
  const { data } = await API.post(
    `/chat/threads/${encodeURIComponent(peerDid)}/read`,
    {},
    { headers: auth(token) }
  );
  return data;
};

export const followPeer = async (token, did) => {
  const { data } = await API.post(
    "/chat/follow",
    { did },
    { headers: auth(token) }
  );
  return data;
};
