const express = require("express");
const auth = require("../middlewares/auth");
const {
  listThreads,
  getThread,
  sendMessage,
  editMessage,
  deleteMessage,
  reactToMessage,
  markThreadRead,
  followPeer,
} = require("../controllers/chat.controller");

const router = express.Router();

router.get("/threads", auth, listThreads);
router.get("/threads/:peerDid", auth, getThread);
router.post("/threads/:peerDid/messages", auth, sendMessage);
router.post("/threads/:peerDid/read", auth, markThreadRead);
router.patch("/messages/:id", auth, editMessage);
router.delete("/messages/:id", auth, deleteMessage);
router.post("/messages/:id/reactions", auth, reactToMessage);
router.post("/follow", auth, followPeer);

module.exports = router;
