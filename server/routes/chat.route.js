const express = require("express");
const multer = require("multer");
const auth = require("../middlewares/auth");
const { ALLOWED_MIMES, ALLOWED_AUDIO_MIMES } = require("../utils/media");
const {
  listThreads,
  getThread,
  sendMessage,
  editMessage,
  deleteMessage,
  reactToMessage,
  markThreadRead,
  followPeer,
  uploadAttachments,
  uploadVoice,
  createRoom,
  listRooms,
  getRoom,
  sendRoomMessage,
  addRoomMember,
  removeRoomMember,
} = require("../controllers/chat.controller");

// Same constraints as post uploads: 25MB per file, up to 4 per message.
const upload = multer({
  storage: multer.memoryStorage(),
  limits: { fileSize: 25 * 1024 * 1024, files: 4 },
  fileFilter: (_req, file, cb) => {
    if (ALLOWED_MIMES.has(file.mimetype)) cb(null, true);
    else cb(new Error("Unsupported file type"));
  },
});

// Voice notes: smaller cap (8MB), audio mimes only.
const voiceUpload = multer({
  storage: multer.memoryStorage(),
  limits: { fileSize: 8 * 1024 * 1024, files: 1 },
  fileFilter: (_req, file, cb) => {
    // Browser MediaRecorder sometimes labels webm-opus as audio/webm;codecs=opus.
    // Strip the codec parameter before comparing.
    const baseMime = (file.mimetype || "").split(";")[0].trim();
    if (ALLOWED_AUDIO_MIMES.has(baseMime)) {
      file.mimetype = baseMime;
      cb(null, true);
    } else {
      cb(new Error("Unsupported audio type"));
    }
  },
});

const router = express.Router();

router.get("/threads", auth, listThreads);
router.get("/threads/:peerDid", auth, getThread);
router.post("/threads/:peerDid/messages", auth, sendMessage);
router.post("/threads/:peerDid/read", auth, markThreadRead);
router.post("/attachments", auth, upload.array("media", 4), uploadAttachments);
router.post("/voice", auth, voiceUpload.single("audio"), uploadVoice);
router.patch("/messages/:id", auth, editMessage);
router.delete("/messages/:id", auth, deleteMessage);
router.post("/messages/:id/reactions", auth, reactToMessage);
router.post("/follow", auth, followPeer);

// Group chat ("room") routes.
router.get("/rooms", auth, listRooms);
router.post("/rooms", auth, createRoom);
router.get("/rooms/:id", auth, getRoom);
router.post("/rooms/:id/messages", auth, sendRoomMessage);
router.post("/rooms/:id/members", auth, addRoomMember);
router.delete("/rooms/:id/members/:did", auth, removeRoomMember);

module.exports = router;
