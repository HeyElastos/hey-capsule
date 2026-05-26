// Hey Social chat — capsule-only.
//
// Federation model: each DM thread = one Carrier gossip topic, canonical
// (smaller did first), so both sides publish to the same topic regardless
// of who started. Group rooms have their own topic per room id.
//
// Local storage layout under localhost://Users/self/.AppData/LocalHost/Hey/:
//   chat/threads.json                       — sidebar index (peerDid → {last_message, ts})
//   chat/messages/<thread_id>/<msg_id>.json — individual signed events
//   chat/rooms.json                         — list of joined rooms
//   chat/rooms/<id>.json                    — room metadata
//   chat/rooms/<id>/<msg_id>.json           — room messages
//   chat/peers.json                         — known peer profiles cache
//   chat/read.json                          — per-thread read cursors
//
// Reads are local (cached gossip events). Writes do two things: publish
// the signed event over Carrier AND append to local storage so the UI
// shows the message immediately.
//
// Public signatures keep a leading `_token` parameter so existing
// component call sites keep working without modification; the value is
// ignored (capsule mode has no JWT).

import { storage, peer, ipfs } from "../lib/runtime";
import { getKeypair, getDidKey } from "../lib/session";
import { createSignedEvent, verifySignedEvent } from "../lib/events";

const threadIdFor = (didA, didB) => [didA, didB].sort().join("::");
const dmTopic = (didA, didB) => `hey-v0/dm/${threadIdFor(didA, didB)}`;
const roomTopic = (roomId) => `hey-v0/room/${roomId}/msg`;
const roomMetaTopic = (roomId) => `hey-v0/room/${roomId}/meta`;

const newMsgId = () => crypto.randomUUID();
const now = () => Date.now();

// ─── Local index helpers ───────────────────────────────────────────

const readThreadIndex = async () =>
  (await storage.readJson("chat/threads.json")) || {};
const writeThreadIndex = (idx) => storage.writeJson("chat/threads.json", idx);

const readRoomIndex = async () =>
  (await storage.readJson("chat/rooms.json")) || {};
const writeRoomIndex = (idx) => storage.writeJson("chat/rooms.json", idx);

const readMessage = (threadId, msgId) =>
  storage.readJson(`chat/messages/${threadId}/${msgId}.json`);
const writeMessage = (threadId, msgId, message) =>
  storage.writeJson(`chat/messages/${threadId}/${msgId}.json`, message);

const readRoomMessage = (roomId, msgId) =>
  storage.readJson(`chat/rooms/${roomId}/${msgId}.json`);
const writeRoomMessage = (roomId, msgId, message) =>
  storage.writeJson(`chat/rooms/${roomId}/${msgId}.json`, message);

const readPeerCache = async () =>
  (await storage.readJson("chat/peers.json")) || {};
const writePeerCache = (cache) => storage.writeJson("chat/peers.json", cache);

const peerDisplay = async (did) => {
  if (!did) return { did: "", name: "", avatar: "" };
  const cache = await readPeerCache();
  const entry = cache[did];
  return {
    did,
    name: entry?.name || `${did.slice(0, 16)}…`,
    avatar: entry?.avatar || "",
  };
};

// Convert a stored event into the wire shape the Chat UI expects.
const toUiMessage = (m) => {
  const payload = m.payload || {};
  return {
    id: payload.id || m.id || m.signature?.slice(0, 16),
    thread_id: payload.thread_id || null,
    room_id: payload.room_id || null,
    sender_did: m.sender_did,
    recipient_did: payload.recipient_did || null,
    content: payload.deleted_at ? null : payload.content,
    ts: payload.ts || m.ts,
    signature: m.signature || null,
    reply_to: payload.reply_to || null,
    reactions: payload.reactions || {},
    attachments: payload.attachments || [],
    read_at: payload.read_at || null,
    edited_at: payload.edited_at || null,
    deleted_at: payload.deleted_at || null,
  };
};

// ─── Sign + publish a chat event ───────────────────────────────────

const signAndPublish = async ({ topic, type, payload }) => {
  const kp = getKeypair();
  if (!kp) throw new Error("No session — sign in first");
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

// ─── DMs ─────────────────────────────────────────────────────────────

export const listThreads = async (_token) => {
  const idx = await readThreadIndex();
  const entries = Object.entries(idx).map(([peerDid, v]) => ({ peerDid, ...v }));
  entries.sort((a, b) => b.ts - a.ts);
  return Promise.all(
    entries.map(async (e) => {
      const p = await peerDisplay(e.peerDid);
      return {
        peer_did: e.peerDid,
        peer_name: p.name,
        peer_avatar: p.avatar,
        last_message: e.last_message,
        ts: e.ts,
      };
    })
  );
};

export const getThread = async (_token, peerDid, opts = {}) => {
  const myDid = getDidKey();
  if (!myDid) throw new Error("Not signed in");
  const threadId = threadIdFor(myDid, peerDid);

  const files = await storage.list(`chat/messages/${threadId}`);
  const list = Array.isArray(files?.entries)
    ? files.entries
    : Array.isArray(files)
      ? files
      : [];
  const msgIds = list
    .map((e) => (typeof e === "string" ? e : e.name))
    .filter((n) => n && n.endsWith(".json"))
    .map((n) => n.replace(/\.json$/, ""));

  let messages = await Promise.all(
    msgIds.map((id) => readMessage(threadId, id))
  );
  messages = messages.filter(Boolean).map(toUiMessage).sort((a, b) => a.ts - b.ts);

  if (opts.before) messages = messages.filter((m) => m.ts < opts.before);
  if (opts.limit) messages = messages.slice(-opts.limit);

  return { peer: await peerDisplay(peerDid), messages };
};

export const sendMessage = async (_token, peerDid, content, replyTo = null, attachments = []) => {
  const myDid = getDidKey();
  if (!myDid) throw new Error("Not signed in");
  const threadId = threadIdFor(myDid, peerDid);
  const msgId = newMsgId();

  const payload = {
    id: msgId,
    thread_id: threadId,
    recipient_did: peerDid,
    content: (content || "").trim(),
    reply_to: replyTo,
    attachments: attachments || [],
    ts: now(),
  };

  const event = await signAndPublish({
    topic: dmTopic(myDid, peerDid),
    type: "chat.msg",
    payload,
  });

  await writeMessage(threadId, msgId, event);
  const idx = await readThreadIndex();
  idx[peerDid] = {
    last_message: content || (attachments.length ? "📎 attachment" : ""),
    ts: payload.ts,
  };
  await writeThreadIndex(idx);

  return toUiMessage(event);
};

// ─── Edit / delete / react (all are signed events on the same topic) ──

const findMessageOwnedByMe = async (messageId) => {
  const myDid = getDidKey();
  if (!myDid) throw new Error("Not signed in");
  const idx = await readThreadIndex();
  for (const peerDid of Object.keys(idx)) {
    const threadId = threadIdFor(myDid, peerDid);
    const original = await readMessage(threadId, messageId);
    if (original) return { peerDid, threadId, original, myDid };
  }
  return null;
};

export const editMessage = async (_token, messageId, content) => {
  const found = await findMessageOwnedByMe(messageId);
  if (!found) throw new Error("Message not found");
  const { peerDid, threadId, original, myDid } = found;
  if (original.sender_did !== myDid) throw new Error("Not your message");
  const updatedPayload = {
    ...original.payload,
    content: content.trim(),
    edited_at: now(),
  };
  const event = await signAndPublish({
    topic: dmTopic(myDid, peerDid),
    type: "chat.edit",
    payload: updatedPayload,
  });
  await writeMessage(threadId, messageId, event);
  return toUiMessage(event);
};

export const deleteMessage = async (_token, messageId) => {
  const found = await findMessageOwnedByMe(messageId);
  if (!found) throw new Error("Message not found");
  const { peerDid, threadId, original, myDid } = found;
  if (original.sender_did !== myDid) throw new Error("Not your message");
  const tombstone = {
    ...original.payload,
    content: null,
    attachments: [],
    reactions: {},
    deleted_at: now(),
  };
  const event = await signAndPublish({
    topic: dmTopic(myDid, peerDid),
    type: "chat.delete",
    payload: tombstone,
  });
  await writeMessage(threadId, messageId, event);
  return toUiMessage(event);
};

export const reactToMessage = async (_token, messageId, emoji) => {
  const found = await findMessageOwnedByMe(messageId);
  if (!found) throw new Error("Message not found");
  const { peerDid, threadId, original, myDid } = found;
  const reactions = { ...(original.payload?.reactions || {}) };
  const list = reactions[emoji] || [];
  const i = list.indexOf(myDid);
  if (i >= 0) list.splice(i, 1);
  else list.push(myDid);
  if (list.length === 0) delete reactions[emoji];
  else reactions[emoji] = list;
  const payload = { ...original.payload, reactions };
  const event = await signAndPublish({
    topic: dmTopic(myDid, peerDid),
    type: "chat.react",
    payload,
  });
  await writeMessage(threadId, messageId, event);
  return toUiMessage(event);
};

export const markThreadRead = async (_token, peerDid) => {
  const read = (await storage.readJson("chat/read.json")) || {};
  read[peerDid] = now();
  await storage.writeJson("chat/read.json", read);
  return { marked: 1 };
};

// ─── Follow / start a DM thread ─────────────────────────────────────

export const followPeer = async (_token, did) => {
  const myDid = getDidKey();
  if (!myDid) throw new Error("Not signed in");
  if (!did || !did.startsWith("did:key:z")) throw new Error("Invalid did:key");
  if (did === myDid) throw new Error("Cannot follow yourself");

  await peer.joinTopic(dmTopic(myDid, did));

  const idx = await readThreadIndex();
  if (!idx[did]) {
    idx[did] = { last_message: "", ts: now() };
    await writeThreadIndex(idx);
  }

  const cache = await readPeerCache();
  if (!cache[did]) {
    cache[did] = { name: `${did.slice(0, 16)}…`, avatar: "" };
    await writePeerCache(cache);
  }

  return { did, name: cache[did].name, avatar: cache[did].avatar, local: false };
};

// ─── Rooms (group chats) ─────────────────────────────────────────────

export const listRooms = async (_token) => {
  const idx = await readRoomIndex();
  return Object.values(idx).sort((a, b) => (b.ts || 0) - (a.ts || 0));
};

export const createRoom = async (_token, name, memberDids) => {
  const myDid = getDidKey();
  if (!myDid) throw new Error("Not signed in");
  const roomId = crypto.randomUUID();
  const room = {
    id: roomId,
    name,
    creator_did: myDid,
    creator_name: "",
    member_dids: Array.from(new Set([myDid, ...(memberDids || [])])),
    member_count: 0,
    avatar: "",
    created_at: now(),
    ts: now(),
    last_message: "",
  };
  room.member_count = room.member_dids.length;

  await peer.joinTopic(roomTopic(roomId));
  await peer.joinTopic(roomMetaTopic(roomId));
  await signAndPublish({
    topic: roomMetaTopic(roomId),
    type: "room.create",
    payload: room,
  });

  const idx = await readRoomIndex();
  idx[roomId] = room;
  await writeRoomIndex(idx);
  await storage.writeJson(`chat/rooms/${roomId}.json`, room);
  return room;
};

export const getRoom = async (_token, roomId, opts = {}) => {
  const room = await storage.readJson(`chat/rooms/${roomId}.json`);
  if (!room) throw new Error("Room not found");

  const files = await storage.list(`chat/rooms/${roomId}`);
  const list = Array.isArray(files?.entries) ? files.entries : Array.isArray(files) ? files : [];
  const msgIds = list
    .map((e) => (typeof e === "string" ? e : e.name))
    .filter((n) => n && n.endsWith(".json") && n !== `${roomId}.json`)
    .map((n) => n.replace(/\.json$/, ""));

  let messages = await Promise.all(msgIds.map((id) => readRoomMessage(roomId, id)));
  messages = messages.filter(Boolean).map(toUiMessage).sort((a, b) => a.ts - b.ts);
  if (opts.before) messages = messages.filter((m) => m.ts < opts.before);
  if (opts.limit) messages = messages.slice(-opts.limit);

  const members = {};
  for (const did of room.member_dids) {
    members[did] = await peerDisplay(did);
  }
  return { room, members, messages };
};

export const sendRoomMessage = async (_token, roomId, content, replyTo = null, attachments = []) => {
  const msgId = newMsgId();
  const payload = {
    id: msgId,
    room_id: roomId,
    content: (content || "").trim(),
    reply_to: replyTo,
    attachments: attachments || [],
    ts: now(),
  };
  const event = await signAndPublish({
    topic: roomTopic(roomId),
    type: "room.msg",
    payload,
  });
  await writeRoomMessage(roomId, msgId, event);
  const idx = await readRoomIndex();
  if (idx[roomId]) {
    idx[roomId].last_message = content || (attachments.length ? "📎 attachment" : "");
    idx[roomId].ts = payload.ts;
    await writeRoomIndex(idx);
  }
  return toUiMessage(event);
};

export const addRoomMember = async (_token, roomId, did) => {
  const room = await storage.readJson(`chat/rooms/${roomId}.json`);
  if (!room) throw new Error("Room not found");
  if (!room.member_dids.includes(did)) {
    room.member_dids.push(did);
    room.member_count = room.member_dids.length;
    await storage.writeJson(`chat/rooms/${roomId}.json`, room);
    await signAndPublish({
      topic: roomMetaTopic(roomId),
      type: "room.member_add",
      payload: { room_id: roomId, did, ts: now() },
    });
  }
  return room;
};

export const leaveRoom = async (_token, roomId, did) => {
  const room = await storage.readJson(`chat/rooms/${roomId}.json`);
  if (!room) return null;
  room.member_dids = room.member_dids.filter((d) => d !== did);
  room.member_count = room.member_dids.length;
  await storage.writeJson(`chat/rooms/${roomId}.json`, room);
  await signAndPublish({
    topic: roomMetaTopic(roomId),
    type: "room.member_remove",
    payload: { room_id: roomId, did, ts: now() },
  });
  return room;
};

// ─── Attachments (photos/videos) via IPFS ──────────────────────────

export const uploadAttachments = async (_token, files, onProgress) => {
  const { transcoder } = await import("../lib/runtime");
  const results = [];
  for (let i = 0; i < files.length; i++) {
    const f = files[i];
    const { blob, format } = await transcoder.processForUpload(f);
    const resp = await ipfs.addBytes(blob, f.name || "file", true);
    const cid = resp?.data?.cid || resp?.cid;
    results.push({
      url: `elastos://${cid}`,
      cid,
      type: f.type?.startsWith("video/") ? "video" : "photo",
      mime: format ? `${f.type?.split("/")[0] || "application"}/${format}` : f.type,
      name: f.name,
    });
    if (onProgress) onProgress(Math.round(((i + 1) / files.length) * 100));
  }
  return results;
};

export const uploadVoice = async (_token, blob, durationMs) => {
  const { transcoder } = await import("../lib/runtime");
  const file = new File([blob], "voice.webm", { type: blob.type || "audio/webm" });
  // Voice messages get loudness-normalized to -16 LUFS and re-encoded to
  // Opus @ 64 kbps. Tiny + consistent volume across senders.
  const { blob: optimized, format } = await transcoder.processForUpload(file, {
    targetCodec: "opus", bitrateK: 64, normalizeLufs: -16,
  });
  const resp = await ipfs.addBytes(optimized, `voice.${format || "webm"}`, true);
  const cid = resp?.data?.cid || resp?.cid;
  return {
    url: `elastos://${cid}`,
    cid,
    type: "voice",
    mime: format ? `audio/${format}` : file.type,
    duration_ms: Math.round(durationMs || 0),
  };
};

// ─── Inbound poll — called periodically by Chat.jsx ────────────────
//
// Pulls new gossip events for every subscribed topic, verifies the
// signature, and writes to local storage so the next listThreads /
// getThread reads them.

export const pollInbound = async () => {
  const myDid = getDidKey();
  if (!myDid) return;
  const idx = await readThreadIndex();
  for (const peerDid of Object.keys(idx)) {
    try {
      const resp = await peer.recv({
        topic: dmTopic(myDid, peerDid),
        limit: 50,
        consumer_id: "hey",
        skip_sender_id: myDid,
      });
      const messages = resp?.data?.messages || [];
      for (const m of messages) {
        let event;
        try { event = JSON.parse(m.message); } catch { continue; }
        const check = verifySignedEvent(event);
        if (!check.valid) continue;
        const threadId = threadIdFor(myDid, peerDid);
        const msgId = event.payload?.id || event.signature.slice(0, 16);
        await writeMessage(threadId, msgId, event);
        idx[peerDid].last_message = event.payload?.content || "";
        idx[peerDid].ts = event.ts;
      }
    } catch { /* topic not joined yet or recv failed — ignore */ }
  }
  await writeThreadIndex(idx);
};
