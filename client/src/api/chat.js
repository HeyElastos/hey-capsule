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

export const sendMessage = async (token, peerDid, content, replyTo = null, attachments = []) => {
  const body = { content };
  if (replyTo) body.reply_to = replyTo;
  if (attachments && attachments.length) body.attachments = attachments;
  const { data } = await API.post(
    `/chat/threads/${encodeURIComponent(peerDid)}/messages`,
    body,
    { headers: auth(token) }
  );
  return data.message;
};

export const uploadAttachments = async (token, files, onProgress) => {
  const form = new FormData();
  for (const f of files) form.append("media", f);
  const { data } = await API.post("/chat/attachments", form, {
    headers: { ...auth(token), "Content-Type": "multipart/form-data" },
    onUploadProgress: (e) => {
      if (onProgress && e.total) onProgress(Math.round((e.loaded / e.total) * 100));
    },
  });
  return data.attachments || [];
};

export const uploadVoice = async (token, blob, durationMs, onProgress) => {
  const form = new FormData();
  // The blob's type is what MediaRecorder gave us ("audio/webm;codecs=opus"
  // typically). Multer strips the codec parameter server-side.
  const file = new File([blob], "voice.webm", { type: blob.type || "audio/webm" });
  form.append("audio", file);
  if (Number.isFinite(durationMs)) form.append("duration_ms", String(Math.round(durationMs)));
  const { data } = await API.post("/chat/voice", form, {
    headers: { ...auth(token), "Content-Type": "multipart/form-data" },
    onUploadProgress: (e) => {
      if (onProgress && e.total) onProgress(Math.round((e.loaded / e.total) * 100));
    },
  });
  return data.attachment;
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

export const listRooms = async (token) => {
  const { data } = await API.get("/chat/rooms", { headers: auth(token) });
  return data.rooms || [];
};

export const createRoom = async (token, name, memberDids) => {
  const { data } = await API.post(
    "/chat/rooms",
    { name, member_dids: memberDids },
    { headers: auth(token) }
  );
  return data.room;
};

export const getRoom = async (token, roomId, opts = {}) => {
  const params = new URLSearchParams();
  if (opts.before) params.set("before", String(opts.before));
  if (opts.limit) params.set("limit", String(opts.limit));
  const qs = params.toString() ? `?${params}` : "";
  const { data } = await API.get(`/chat/rooms/${encodeURIComponent(roomId)}${qs}`, {
    headers: auth(token),
  });
  return data;
};

export const sendRoomMessage = async (token, roomId, content, replyTo = null, attachments = []) => {
  const body = { content };
  if (replyTo) body.reply_to = replyTo;
  if (attachments && attachments.length) body.attachments = attachments;
  const { data } = await API.post(
    `/chat/rooms/${encodeURIComponent(roomId)}/messages`,
    body,
    { headers: auth(token) }
  );
  return data.message;
};

export const addRoomMember = async (token, roomId, did) => {
  const { data } = await API.post(
    `/chat/rooms/${encodeURIComponent(roomId)}/members`,
    { did },
    { headers: auth(token) }
  );
  return data.room;
};

export const leaveRoom = async (token, roomId, did) => {
  const { data } = await API.delete(
    `/chat/rooms/${encodeURIComponent(roomId)}/members/${encodeURIComponent(did)}`,
    { headers: auth(token) }
  );
  return data.room;
};
