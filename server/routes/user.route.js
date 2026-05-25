const router = require("express").Router();
const multer = require("multer");
const {
  signup,
  signin,
  me,
  updateMe,
  deleteMe,
  getUserById,
  requestFollow,
  cancelFollowRequest,
  acceptFollow,
  rejectFollow,
} = require("../controllers/user.controller");
const requireAuth = require("../middlewares/auth");
const optionalAuth = require("../middlewares/optionalAuth");

const upload = multer({
  storage: multer.memoryStorage(),
  limits: { fileSize: 10 * 1024 * 1024 },
});

router.post("/signup", signup);
router.post("/signin", signin);
router.get("/me", requireAuth, me);
router.patch("/me", requireAuth, upload.single("avatar"), updateMe);
router.delete("/me", requireAuth, deleteMe);
router.get("/:id", optionalAuth, getUserById);
router.post("/:id/follow", requireAuth, requestFollow);
router.delete("/:id/follow", requireAuth, cancelFollowRequest);
router.post("/:id/follow/accept", requireAuth, acceptFollow);
router.post("/:id/follow/reject", requireAuth, rejectFollow);

module.exports = router;
