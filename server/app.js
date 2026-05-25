require("dotenv").config();
const express = require("express");
const cors = require("cors");
const path = require("path");
const helmet = require("helmet");
const rateLimit = require("express-rate-limit");
const userRoutes = require("./routes/user.route");
const postRoutes = require("./routes/post.route");
const notificationRoutes = require("./routes/notification.route");
const passkeyRoutes = require("./routes/passkey.route");

const app = express();
const PORT = process.env.PORT || 4000;

const isProd = process.env.NODE_ENV === "production";
const CLIENT_ORIGIN =
  process.env.CLIENT_ORIGIN || (isProd ? "" : "http://localhost:3000");

if (isProd && !CLIENT_ORIGIN) {
  throw new Error("CLIENT_ORIGIN is required in production");
}

// Trust one hop of reverse proxy (rate-limit needs the real client IP).
app.set("trust proxy", 1);

// Strict security headers + CSP. Uploaded files served below get
// X-Content-Type-Options: nosniff via the same helmet pass.
app.use(
  helmet({
    crossOriginResourcePolicy: { policy: "cross-origin" },
    contentSecurityPolicy: false, // SPA served separately by Vite; CSP applies via the SPA host.
  })
);

app.use(
  cors({
    origin: CLIENT_ORIGIN,
    credentials: true,
  })
);

app.use(express.json({ limit: "100kb" }));

// Force download for any uploaded file → even if a malicious file with an
// HTML/SVG payload slips past validation, browsers won't execute it.
app.use(
  "/uploads",
  (req, res, next) => {
    res.setHeader("X-Content-Type-Options", "nosniff");
    res.setHeader("Content-Disposition", "inline");
    next();
  },
  express.static(path.join(__dirname, "uploads"), {
    dotfiles: "deny",
    fallthrough: false,
    setHeaders: (res) => {
      res.setHeader("X-Content-Type-Options", "nosniff");
    },
  })
);

// Rate limiters
const authLimiter = rateLimit({
  windowMs: 15 * 60 * 1000,
  max: 20,
  standardHeaders: true,
  legacyHeaders: false,
  message: { message: "Too many attempts, try again later." },
});
const writeLimiter = rateLimit({
  windowMs: 60 * 1000,
  max: 60,
  standardHeaders: true,
  legacyHeaders: false,
  message: { message: "Slow down, too many requests." },
});
const uploadLimiter = rateLimit({
  windowMs: 60 * 60 * 1000,
  max: 30,
  standardHeaders: true,
  legacyHeaders: false,
  message: { message: "Upload limit reached, try later." },
});

app.get("/server-status", (req, res) => {
  res.status(200).json({ status: "ok", message: "Server is running" });
});

// Per-router middlewares
app.use("/users/signup", authLimiter);
app.use("/users/signin", authLimiter);
app.use("/passkey", authLimiter);
app.use("/posts", (req, res, next) => {
  // Only limit mutating routes; reads pass through.
  if (req.method === "POST" && req.path === "/") return uploadLimiter(req, res, next);
  if (req.method !== "GET") return writeLimiter(req, res, next);
  next();
});
app.use("/notifications", writeLimiter);
app.use("/users", (req, res, next) => {
  // POST/PATCH/DELETE on user routes (excluding signup/signin handled above)
  if (req.method === "GET") return next();
  return writeLimiter(req, res, next);
});

app.use("/users", userRoutes);
app.use("/posts", postRoutes);
app.use("/notifications", notificationRoutes);
app.use("/passkey", passkeyRoutes);

app.use((req, res) => {
  res.status(404).json({ message: "Route not found" });
});

// Centralized error handler — never leak `error.message`/stack to clients.
// eslint-disable-next-line no-unused-vars
app.use((err, req, res, _next) => {
  console.error("[error]", req.method, req.url, err);
  // Multer file-size / fileFilter errors are user-facing
  if (err && err.code === "LIMIT_FILE_SIZE") {
    return res.status(413).json({ message: "File too large" });
  }
  if (err && err.message === "Unsupported file type") {
    return res.status(415).json({ message: "Unsupported file type" });
  }
  res.status(500).json({ message: "Something went wrong" });
});

app.listen(PORT, () => {
  console.log(`Server up and running on port ${PORT}!`);
});
