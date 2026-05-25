// Chat controller — DM-only, did:key-addressed.
//
// Phase 2: works between any two accounts on the same Hey instance.
// Phase 3: identical API; transport layer swaps to Carrier gossip so messages
//          can cross instances. The on-wire shape of each message already
//          matches the SignedEvent envelope, just with signature=null in
//          local mode (server-attested) and signature filled in by the
//          sender for federated mode.
//
// REST surface:
//   GET    /chat/threads                          → list of threads with last message
//   GET    /chat/threads/:peerDid                 → messages for one thread
//   POST   /chat/threads/:peerDid/messages        → send a message
//   POST   /chat/follow                           → add a peer by did:key

const crypto = require("crypto");
const fs = require("fs/promises");
const { readDb, writeDb } = require("../utils/db");
const env = require("../utils/env");
const { processFile } = require("../utils/media");

const MAX_CONTENT_LEN = 2000;
const MAX_ATTACHMENTS_PER_MESSAGE = 4;
const PAGE_LIMIT = 100;
const EDIT_WINDOW_MS = 15 * 60 * 1000;
const DEFAULT_REACTIONS = new Set(["❤️", "🔥", "😂", "😮", "😢", "👏", "💯", "✨"]);

// Find the local user record that owns a did:key. Returns undefined for
// did:keys that don't map to any local account (Phase 3 federation will
// expand this to lookup remote peers).
const userByDid = (db, did) => db.users.find((u) => u.didKey === did);

// Make a canonical thread id from a pair of dids. Sorted so the same two
// people produce the same thread regardless of who started it.
const threadIdFor = (didA, didB) => [didA, didB].sort().join("::");

// Public-shaped message for the wire. Mirrors the SignedEvent envelope so
// Phase 3 can move to gossip transport without re-shaping the JSON.
const toPublicMessage = (m) => ({
  id: m.id,
  thread_id: m.threadId,
  sender_did: m.senderDid,
  recipient_did: m.recipientDid,
  content: m.deletedAt ? null : m.content,
  ts: m.ts,
  signature: m.signature || null,
  reply_to: m.replyTo || null,
  reactions: m.reactions || {},
  attachments: m.deletedAt ? [] : (m.attachments || []),
  read_at: m.readAt || null,
  edited_at: m.editedAt || null,
  deleted_at: m.deletedAt || null,
});

// POST /chat/attachments — accepts up to MAX_ATTACHMENTS_PER_MESSAGE files
// via multer (memory storage), runs each through the shared media pipeline
// (magic-byte check + sharp/ffmpeg transcoding), returns { attachments: [...] }
// for the client to attach to a subsequent sendMessage call.
//
// Decoupling upload from send lets the client show real previews
// (post-transcode dimensions) and lets the user remove attachments before
// finalizing the message.
const uploadAttachments = async (req, res) => {
  try {
    const db = await readDb();
    const me = db.users.find((u) => u.id === req.user.id);
    if (!me?.didKey) {
      return res.status(409).json({ message: "Identity not ready" });
    }
    const files = req.files || [];
    if (files.length === 0) {
      return res.status(400).json({ message: "No files uploaded" });
    }
    if (files.length > MAX_ATTACHMENTS_PER_MESSAGE) {
      return res.status(400).json({
        message: `Max ${MAX_ATTACHMENTS_PER_MESSAGE} attachments per message`,
      });
    }

    const uploadsDir = env.UPLOADS_DIR;
    await fs.mkdir(uploadsDir, { recursive: true });
    const attachments = await Promise.all(
      files.map((f) => processFile(f, uploadsDir))
    );
    return res.status(201).json({ attachments });
  } catch (e) {
    if (e?.message?.startsWith("Disallowed file type")) {
      return res.status(415).json({ message: e.message });
    }
    return res.status(500).json({ message: "Upload failed" });
  }
};

// Validate a client-supplied attachments array against the post-upload shape.
// We trust the URLs from the prior uploadAttachments response because those
// files live in our own /uploads dir; we just defensively validate shape so
// a malicious client can't stuff arbitrary metadata into the message.
const sanitizeAttachments = (input) => {
  if (!Array.isArray(input)) return [];
  return input
    .slice(0, MAX_ATTACHMENTS_PER_MESSAGE)
    .map((a) => {
      if (!a || typeof a !== "object") return null;
      if (typeof a.url !== "string" || !a.url.startsWith("/uploads/")) return null;
      if (a.type !== "photo" && a.type !== "video") return null;
      const out = { url: a.url, type: a.type };
      if (a.type === "photo") {
        if (Number.isInteger(a.width) && a.width > 0) out.width = a.width;
        if (Number.isInteger(a.height) && a.height > 0) out.height = a.height;
      } else {
        out.mime = "video/mp4";
      }
      return out;
    })
    .filter(Boolean);
};

const findMessage = (db, id) => db.chatMessages.find((m) => m.id === id);

// GET /chat/threads — list every thread the caller participates in,
// newest-message-first, with last-message preview.
const listThreads = async (req, res) => {
  try {
    const db = await readDb();
    const me = db.users.find((u) => u.id === req.user.id);
    if (!me) return res.status(404).json({ message: "User not found" });
    if (!me.didKey) {
      return res.status(409).json({
        message: "Your account is missing a federation identity — sign out and back in to backfill it.",
      });
    }

    const myDid = me.didKey;
    const threadMap = new Map();

    for (const m of db.chatMessages) {
      if (m.senderDid !== myDid && m.recipientDid !== myDid) continue;
      const peerDid = m.senderDid === myDid ? m.recipientDid : m.senderDid;
      const existing = threadMap.get(peerDid);
      if (!existing || existing.ts < m.ts) {
        let preview = m.content;
        if (m.deletedAt) {
          preview = "(message deleted)";
        } else if (!preview && Array.isArray(m.attachments) && m.attachments.length > 0) {
          const a = m.attachments[0];
          preview = a.type === "video" ? "📹 Video" : "📷 Photo";
          if (m.attachments.length > 1) preview += ` (+${m.attachments.length - 1})`;
        }
        threadMap.set(peerDid, { peerDid, lastMessage: preview || "", ts: m.ts });
      }
    }

    // Resolve display info for peer dids that map to local accounts. Unknown
    // dids (federated peers we don't have a local record for) come back with
    // truncated did as the display name.
    const threads = [...threadMap.values()]
      .sort((a, b) => b.ts - a.ts)
      .map(({ peerDid, lastMessage, ts }) => {
        const peer = userByDid(db, peerDid);
        return {
          peer_did: peerDid,
          peer_name: peer?.name || `${peerDid.slice(0, 16)}…`,
          peer_avatar: peer?.avatar || "",
          last_message: lastMessage,
          ts,
        };
      });

    return res.status(200).json({ threads });
  } catch {
    return res.status(500).json({ message: "Failed to load threads" });
  }
};

// GET /chat/threads/:peerDid — paginated message history with one peer.
// ?before=<ts>&limit=<n>
const getThread = async (req, res) => {
  try {
    const db = await readDb();
    const me = db.users.find((u) => u.id === req.user.id);
    if (!me?.didKey) return res.status(404).json({ message: "Identity not ready" });

    const peerDid = req.params.peerDid;
    if (!peerDid || !peerDid.startsWith("did:key:z")) {
      return res.status(400).json({ message: "Invalid peer did" });
    }

    const before = req.query.before ? Number(req.query.before) : Infinity;
    const limit = Math.min(Number(req.query.limit) || PAGE_LIMIT, PAGE_LIMIT);

    const tid = threadIdFor(me.didKey, peerDid);
    const messages = db.chatMessages
      .filter((m) => m.threadId === tid && m.ts < before)
      .sort((a, b) => a.ts - b.ts)
      .slice(-limit)
      .map(toPublicMessage);

    const peer = userByDid(db, peerDid);
    return res.status(200).json({
      peer: {
        did: peerDid,
        name: peer?.name || `${peerDid.slice(0, 16)}…`,
        avatar: peer?.avatar || "",
      },
      messages,
    });
  } catch {
    return res.status(500).json({ message: "Failed to load thread" });
  }
};

// POST /chat/threads/:peerDid/messages — send a message. Body: { content }.
// Phase 2 stores server-side with signature=null (local-mode). Phase 3 will
// accept a client-supplied signature and verify before storing.
const sendMessage = async (req, res) => {
  try {
    const db = await readDb();
    const me = db.users.find((u) => u.id === req.user.id);
    if (!me?.didKey) {
      return res.status(409).json({ message: "Sign out and back in to enable chat." });
    }

    const peerDid = req.params.peerDid;
    if (!peerDid || !peerDid.startsWith("did:key:z")) {
      return res.status(400).json({ message: "Invalid peer did" });
    }
    if (peerDid === me.didKey) {
      return res.status(400).json({ message: "You can't message yourself" });
    }

    const content = typeof req.body?.content === "string" ? req.body.content.trim() : "";
    const attachments = sanitizeAttachments(req.body?.attachments);
    if (!content && attachments.length === 0) {
      return res.status(400).json({ message: "Message can't be empty" });
    }
    if (content.length > MAX_CONTENT_LEN) {
      return res.status(413).json({ message: `Message exceeds ${MAX_CONTENT_LEN} chars` });
    }

    // Validate reply target: must exist and belong to the same thread.
    let replyTo = null;
    if (req.body?.reply_to) {
      const target = findMessage(db, req.body.reply_to);
      if (target && target.threadId === threadIdFor(me.didKey, peerDid)) {
        replyTo = target.id;
      }
    }

    const message = {
      id: crypto.randomUUID(),
      threadId: threadIdFor(me.didKey, peerDid),
      senderDid: me.didKey,
      recipientDid: peerDid,
      content,
      ts: Date.now(),
      signature: null, // Phase 3: filled by client-side signer
      replyTo,
      reactions: {},
      attachments,
      readAt: null,
      editedAt: null,
      deletedAt: null,
    };

    db.chatMessages.push(message);
    await writeDb(db);

    return res.status(201).json({ message: toPublicMessage(message) });
  } catch {
    return res.status(500).json({ message: "Failed to send message" });
  }
};

// PATCH /chat/messages/:id — edit a message you sent, within EDIT_WINDOW_MS.
const editMessage = async (req, res) => {
  try {
    const db = await readDb();
    const me = db.users.find((u) => u.id === req.user.id);
    if (!me?.didKey) return res.status(409).json({ message: "Identity not ready" });

    const msg = findMessage(db, req.params.id);
    if (!msg) return res.status(404).json({ message: "Message not found" });
    if (msg.senderDid !== me.didKey) {
      return res.status(403).json({ message: "Not your message" });
    }
    if (msg.deletedAt) {
      return res.status(410).json({ message: "Message was deleted" });
    }
    if (Date.now() - msg.ts > EDIT_WINDOW_MS) {
      return res.status(403).json({ message: "Edit window expired" });
    }

    const content = typeof req.body?.content === "string" ? req.body.content.trim() : "";
    if (!content) return res.status(400).json({ message: "Message can't be empty" });
    if (content.length > MAX_CONTENT_LEN) {
      return res.status(413).json({ message: `Message exceeds ${MAX_CONTENT_LEN} chars` });
    }
    if (content === msg.content) {
      // No-op edit; don't bump editedAt.
      return res.status(200).json({ message: toPublicMessage(msg) });
    }

    msg.content = content;
    msg.editedAt = Date.now();
    await writeDb(db);
    return res.status(200).json({ message: toPublicMessage(msg) });
  } catch {
    return res.status(500).json({ message: "Failed to edit message" });
  }
};

// DELETE /chat/messages/:id — soft-delete a message you sent. The row stays
// so reply threads don't lose context; the content is just redacted.
const deleteMessage = async (req, res) => {
  try {
    const db = await readDb();
    const me = db.users.find((u) => u.id === req.user.id);
    if (!me?.didKey) return res.status(409).json({ message: "Identity not ready" });

    const msg = findMessage(db, req.params.id);
    if (!msg) return res.status(404).json({ message: "Message not found" });
    if (msg.senderDid !== me.didKey) {
      return res.status(403).json({ message: "Not your message" });
    }
    if (msg.deletedAt) {
      return res.status(200).json({ message: toPublicMessage(msg) });
    }

    msg.deletedAt = Date.now();
    msg.reactions = {}; // reactions on a deleted message no longer make sense
    await writeDb(db);
    return res.status(200).json({ message: toPublicMessage(msg) });
  } catch {
    return res.status(500).json({ message: "Failed to delete message" });
  }
};

// POST /chat/messages/:id/reactions — toggle a reaction on or off.
// Body: { emoji }. Only allowed if you're a participant in the thread.
const reactToMessage = async (req, res) => {
  try {
    const db = await readDb();
    const me = db.users.find((u) => u.id === req.user.id);
    if (!me?.didKey) return res.status(409).json({ message: "Identity not ready" });

    const msg = findMessage(db, req.params.id);
    if (!msg) return res.status(404).json({ message: "Message not found" });
    if (msg.deletedAt) return res.status(410).json({ message: "Message was deleted" });

    // Only thread participants may react.
    if (msg.senderDid !== me.didKey && msg.recipientDid !== me.didKey) {
      return res.status(403).json({ message: "Not your thread" });
    }

    const emoji = typeof req.body?.emoji === "string" ? req.body.emoji : "";
    if (!DEFAULT_REACTIONS.has(emoji)) {
      return res.status(400).json({ message: "Unsupported emoji" });
    }

    if (!msg.reactions) msg.reactions = {};
    const list = msg.reactions[emoji] || [];
    const idx = list.indexOf(me.didKey);
    if (idx >= 0) {
      list.splice(idx, 1);
    } else {
      list.push(me.didKey);
    }
    if (list.length === 0) delete msg.reactions[emoji];
    else msg.reactions[emoji] = list;

    await writeDb(db);
    return res.status(200).json({ message: toPublicMessage(msg) });
  } catch {
    return res.status(500).json({ message: "Failed to react" });
  }
};

// POST /chat/threads/:peerDid/read — mark every message from the peer as read.
// Returns the count of messages newly marked.
const markThreadRead = async (req, res) => {
  try {
    const db = await readDb();
    const me = db.users.find((u) => u.id === req.user.id);
    if (!me?.didKey) return res.status(409).json({ message: "Identity not ready" });

    const peerDid = req.params.peerDid;
    if (!peerDid || !peerDid.startsWith("did:key:z")) {
      return res.status(400).json({ message: "Invalid peer did" });
    }

    const tid = threadIdFor(me.didKey, peerDid);
    const now = Date.now();
    let marked = 0;
    for (const m of db.chatMessages) {
      if (m.threadId !== tid) continue;
      if (m.senderDid !== peerDid) continue; // only mark inbound msgs
      if (m.readAt) continue;
      m.readAt = now;
      marked++;
    }
    if (marked > 0) await writeDb(db);
    return res.status(200).json({ marked });
  } catch {
    return res.status(500).json({ message: "Failed to mark read" });
  }
};

// POST /chat/follow — record a peer did so it shows up as a contact. In
// Phase 2 this is just a sanity-check shortcut to bootstrap a conversation
// before either side has sent a message yet. Phase 3 will tie this to the
// Carrier gossip_join call.
const followPeer = async (req, res) => {
  try {
    const db = await readDb();
    const me = db.users.find((u) => u.id === req.user.id);
    if (!me?.didKey) {
      return res.status(409).json({ message: "Sign out and back in to enable chat." });
    }

    const peerDid = typeof req.body?.did === "string" ? req.body.did.trim() : "";
    if (!peerDid.startsWith("did:key:z")) {
      return res.status(400).json({ message: "Invalid did:key" });
    }
    if (peerDid === me.didKey) {
      return res.status(400).json({ message: "Cannot follow yourself" });
    }

    const peer = userByDid(db, peerDid);
    return res.status(200).json({
      did: peerDid,
      name: peer?.name || `${peerDid.slice(0, 16)}…`,
      avatar: peer?.avatar || "",
      local: Boolean(peer),
    });
  } catch {
    return res.status(500).json({ message: "Failed to follow peer" });
  }
};

module.exports = {
  listThreads,
  getThread,
  sendMessage,
  editMessage,
  deleteMessage,
  reactToMessage,
  markThreadRead,
  followPeer,
  uploadAttachments,
};
