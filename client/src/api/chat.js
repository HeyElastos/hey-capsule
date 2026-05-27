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
import { encryptToHybrid, decryptHybrid } from "../lib/pqcrypto";
import { resolveBundle } from "../lib/profile";

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
    // True if the message was delivered as a hybrid-PQ envelope and we
    // decrypted it before storage. Lets the UI render a 🔒 badge.
    encrypted: m._was_encrypted === true,
  };
};

// ─── Sign + publish a chat event ───────────────────────────────────
//
// For DMs, pass `recipientDid` — if the peer's profile bundle is
// resolvable, the payload is wrapped in a hybrid-PQ envelope BEFORE
// signing, so only the recipient (and us, via our own KEM key on
// decrypt) can read it. The signature still authenticates the sender.
//
// Returned event has two extra fields the caller can use:
//   _was_encrypted: true|false
//   _plain_payload: the original plaintext payload (always present, so
//                   the local writeMessage call stores the readable
//                   version rather than the ciphertext).
//
// Group / room events that pass through here without a recipientDid
// stay transit-only — same threat model as before, badge will reflect.

const signAndPublish = async ({ topic, type, payload, recipientDid }) => {
  const kp = getKeypair();
  if (!kp) throw new Error("No session — sign in first");

  let onWirePayload = payload;
  let encrypted = false;

  if (recipientDid && kp.x25519?.publicKey && kp.kem?.publicKey) {
    try {
      const bundle = await resolveBundle(recipientDid);
      if (bundle?.x25519Pub && bundle?.kemPub) {
        const env = encryptToHybrid(
          JSON.stringify(payload),
          bundle.x25519Pub,
          bundle.kemPub,
        );
        onWirePayload = { enc: env };
        encrypted = true;
      }
    } catch (err) {
      console.warn("[hey-chat] encrypt failed, falling back to transit-only", err);
    }
  }

  const wireEvent = await createSignedEvent(
    { type, payload: onWirePayload },
    kp,
  );
  await peer.publish({
    topic,
    message: JSON.stringify(wireEvent),
    sender_id: wireEvent.sender_did,
    ts: wireEvent.ts,
    signature: wireEvent.signature,
  });

  // Locally we want the readable event. Return a shape that lets callers
  // persist the plaintext payload + the original signature/ts so
  // toUiMessage continues to surface .signature for UI display.
  return {
    ...wireEvent,
    payload, // plaintext, replaces the on-wire encrypted form
    _was_encrypted: encrypted,
  };
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
    recipientDid: peerDid,
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

// ─── DID groups (no rooms, no owner) ───────────────────────────────
//
// A "group" is a set of DIDs. The gossip topic is deterministic from
// the sorted DID list — anyone holding the same set computes the same
// topic and can publish to it. No room IDs, no admins, no invite
// requests. To "add" someone you broadcast a group.update message
// listing the new DID set; receivers re-derive the new topic and
// re-subscribe. To leave, you stop subscribing.

const groupTopic = (groupId) => `hey-v0/group/${groupId}/msg`;

// sha256(sorted DIDs joined by NUL) → hex. Deterministic ID anyone can
// recompute from the participant list — no central registry needed.
const groupIdFromDids = async (dids) => {
  const sorted = [...new Set(dids.filter(Boolean))].sort();
  const bytes = new TextEncoder().encode(sorted.join("\0"));
  const hash = await crypto.subtle.digest("SHA-256", bytes);
  const hex = Array.from(new Uint8Array(hash))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
  return { groupId: hex, dids: sorted };
};

const readGroupIndex = async () =>
  (await storage.readJson("chat/groups.json")) || {};
const writeGroupIndex = (idx) => storage.writeJson("chat/groups.json", idx);

const readGroupMessage = (groupId, msgId) =>
  storage.readJson(`chat/groups/${groupId}/messages/${msgId}.json`);
const writeGroupMessage = (groupId, msgId, message) =>
  storage.writeJson(`chat/groups/${groupId}/messages/${msgId}.json`, message);

export const listGroups = async (_token) => {
  const idx = await readGroupIndex();
  const entries = Object.entries(idx).map(([groupId, v]) => ({ groupId, ...v }));
  entries.sort((a, b) => b.ts - a.ts);
  return Promise.all(
    entries.map(async (g) => ({
      group_id: g.groupId,
      name: g.name,
      member_dids: g.member_dids,
      members: await Promise.all(g.member_dids.map(peerDisplay)),
      last_message: g.last_message,
      ts: g.ts,
    }))
  );
};

export const createGroup = async (_token, name, memberDids) => {
  const myDid = getDidKey();
  if (!myDid) throw new Error("Not signed in");
  if (!Array.isArray(memberDids) || memberDids.length === 0) {
    throw new Error("createGroup: memberDids required");
  }
  const { groupId, dids } = await groupIdFromDids([myDid, ...memberDids]);
  const topic = groupTopic(groupId);

  // Subscribe so subsequent gossip arrives on pollInbound.
  try { await peer.joinTopic(topic); } catch { /* may already be joined */ }

  // Persist locally.
  const idx = await readGroupIndex();
  idx[groupId] = {
    name: name || "",
    member_dids: dids,
    created_at: idx[groupId]?.created_at || now(),
    ts: now(),
    last_message: idx[groupId]?.last_message || "",
  };
  await writeGroupIndex(idx);

  // Announce so peers' pollInbound can discover the group via their
  // own DID inbox if we wire that, or accept on first message receipt.
  await signAndPublish({
    topic,
    type: "group.upsert",
    payload: { group_id: groupId, name: name || "", member_dids: dids, ts: now() },
  });

  return { group_id: groupId, name: name || "", member_dids: dids };
};

export const getGroup = async (_token, groupId, opts = {}) => {
  const idx = await readGroupIndex();
  const group = idx[groupId];
  if (!group) return null;
  const files = await storage.list(`chat/groups/${groupId}/messages`);
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
    msgIds.map((id) => readGroupMessage(groupId, id))
  );
  messages = messages
    .filter(Boolean)
    .map((m) => ({ ...toUiMessage(m), group_id: groupId }))
    .sort((a, b) => a.ts - b.ts);
  if (opts.before) messages = messages.filter((m) => m.ts < opts.before);
  if (opts.limit) messages = messages.slice(-opts.limit);
  return {
    group: {
      group_id: groupId,
      name: group.name,
      member_dids: group.member_dids,
      members: await Promise.all(group.member_dids.map(peerDisplay)),
    },
    messages,
  };
};

export const sendGroupMessage = async (
  _token, groupId, content, replyTo = null, attachments = []
) => {
  const idx = await readGroupIndex();
  const group = idx[groupId];
  if (!group) throw new Error("Unknown group");
  const msgId = newMsgId();
  const event = await signAndPublish({
    topic: groupTopic(groupId),
    type: "group.message",
    payload: {
      id: msgId,
      group_id: groupId,
      content,
      ts: now(),
      reply_to: replyTo,
      attachments,
    },
  });
  await writeGroupMessage(groupId, msgId, event);
  idx[groupId].last_message = content;
  idx[groupId].ts = event.ts;
  await writeGroupIndex(idx);
  return { ...toUiMessage(event), group_id: groupId };
};

export const addGroupMember = async (_token, groupId, newDid) => {
  // "Adding" a member means re-computing the group topic with the
  // expanded DID set. Sender re-derives, re-subscribes, and broadcasts
  // a group.upsert to the NEW topic listing all members. Receivers
  // verify the topic matches sha256(sorted-dids) and accept.
  const idx = await readGroupIndex();
  const old = idx[groupId];
  if (!old) throw new Error("Unknown group");
  if (old.member_dids.includes(newDid)) return { group_id: groupId };
  return createGroup(_token, old.name, [...old.member_dids.filter((d) => d !== getDidKey()), newDid]);
};

export const leaveGroup = async (_token, groupId) => {
  const idx = await readGroupIndex();
  if (!idx[groupId]) return;
  try { await peer.leaveTopic(groupTopic(groupId)); } catch { /* not joined */ }
  delete idx[groupId];
  await writeGroupIndex(idx);
};

// ─── Attachments (photos/videos) via IPFS ──────────────────────────

export const uploadAttachments = async (_token, files, onProgress) => {
  const { transcoder } = await import("../lib/runtime");
  const results = [];
  for (let i = 0; i < files.length; i++) {
    const f = files[i];
    const { blob, format } = await transcoder.processForUpload(f);
    const resp = await ipfs.addBytes(blob, f.name || "file", true);
    // Defensive: if ipfs.addBytes didn't return a CID (auth failed, kubo
    // down, provider not registered, etc.), throw a clear error here
    // instead of letting downstream code construct 'elastos://undefined'
    // and crash later with an opaque 'n is not a function'.
    const cid = resp?.data?.cid || resp?.cid;
    if (!cid || typeof cid !== "string") {
      throw new Error(
        `IPFS add_bytes returned no CID for ${f.name || "attachment"}` +
        ` (response: ${JSON.stringify(resp).slice(0, 200)})`
      );
    }
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
  if (!cid || typeof cid !== "string") {
    throw new Error(
      `IPFS add_bytes returned no CID for voice clip` +
      ` (response: ${JSON.stringify(resp).slice(0, 200)})`
    );
  }
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

// Try to decrypt a received event. Returns either:
//   { event: <event-with-decrypted-payload>, encrypted: true }   on success
//   { event: original_event,                  encrypted: false } if not encrypted
//   null                                                          on decrypt failure
// The signature on the returned event won't re-verify (payload changed),
// but we trust local storage; we already verified before decryption.
const tryDecryptInbound = (event) => {
  const env = event?.payload?.enc;
  if (!env || env.v !== "hpq-1") {
    return { event, encrypted: false };
  }
  const kp = getKeypair();
  if (!kp?.x25519?.privateKey || !kp?.kem?.secretKey) return null;
  try {
    const plaintext = decryptHybrid(env, kp.x25519.privateKey, kp.kem.secretKey);
    let parsed = plaintext;
    try { parsed = JSON.parse(plaintext); } catch {}
    return {
      event: { ...event, payload: parsed, _was_encrypted: true },
      encrypted: true,
    };
  } catch (err) {
    return null;
  }
};

export const pollInbound = async () => {
  const myDid = getDidKey();
  if (!myDid) return;

  // 1:1 DM threads — one topic per peer DID pair.
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
        const decoded = tryDecryptInbound(event);
        if (!decoded) {
          // Encrypted but we can't read it — persist a stub so the
          // thread index still updates with timestamps and the user
          // sees the placeholder.
          const threadId = threadIdFor(myDid, peerDid);
          const msgId = event.payload?.id || event.signature.slice(0, 16);
          await writeMessage(threadId, msgId, {
            ...event,
            payload: { id: msgId, content: "🔒 encrypted — no key", _unreadable: true, ts: event.ts },
            _was_encrypted: true,
          });
          idx[peerDid].last_message = "🔒 encrypted";
          idx[peerDid].ts = event.ts;
          continue;
        }
        const usable = decoded.event;
        const threadId = threadIdFor(myDid, peerDid);
        const msgId = usable.payload?.id || event.signature.slice(0, 16);
        await writeMessage(threadId, msgId, usable);
        idx[peerDid].last_message = usable.payload?.content || "";
        idx[peerDid].ts = event.ts;
      }
    } catch { /* topic not joined yet or recv failed — ignore */ }
  }
  await writeThreadIndex(idx);

  // Group conversations — one topic per sorted-DID-set hash.
  const groupIdx = await readGroupIndex();
  for (const groupId of Object.keys(groupIdx)) {
    try {
      const resp = await peer.recv({
        topic: groupTopic(groupId),
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
        // group.upsert events update local membership; group.message events
        // get persisted as ordered history.
        if (event.payload?.type === "group.upsert" || event.type === "group.upsert") {
          const next = event.payload?.member_dids;
          if (Array.isArray(next)) {
            // Verify the topic matches sha256(sorted-dids) so a malicious
            // sender can't claim a group has unrelated members.
            const recomputed = await groupIdFromDids(next);
            if (recomputed.groupId === groupId) {
              groupIdx[groupId].member_dids = recomputed.dids;
              groupIdx[groupId].name = event.payload?.name || groupIdx[groupId].name;
            }
          }
          continue;
        }
        const msgId = event.payload?.id || event.signature.slice(0, 16);
        await writeGroupMessage(groupId, msgId, event);
        groupIdx[groupId].last_message = event.payload?.content || "";
        groupIdx[groupId].ts = event.ts;
      }
    } catch { /* not joined yet or recv failed — ignore */ }
  }
  await writeGroupIndex(groupIdx);
};
