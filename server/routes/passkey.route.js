const router = require("express").Router();
const requireAuth = require("../middlewares/auth");
const optionalAuth = require("../middlewares/optionalAuth");
const {
  registerOptions,
  registerVerify,
  authOptions,
  authVerify,
} = require("../controllers/passkey.controller");

router.post("/register/options", optionalAuth, registerOptions);
router.post("/register/verify", optionalAuth, registerVerify);
router.post("/auth/options", authOptions);
router.post("/auth/verify", authVerify);

module.exports = router;
