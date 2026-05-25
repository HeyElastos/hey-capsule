const env = require("./utils/env");
const logger = require("./utils/logger");
const express = require("express");
const cors = require("cors");
const fs = require("fs");
const helmet = require("helmet");
const rateLimit = require("express-rate-limit");
const userRoutes = require("./routes/user.route");
const postRoutes = require("./routes/post.route");
const notificationRoutes = require("./routes/notification.route");
const passkeyRoutes = require("./routes/passkey.route");
const chatRoutes = require("./routes/chat.route");

const app = express();
const PORT = env.PORT;
const CLIENT_ORIGIN = env.CLIENT_ORIGIN;

// Ensure persistent data + uploads dirs exist before any route handler tries
// to write to them. In YunoHost these live outside install_dir so they
// survive `yunohost app upgrade hey`.
fs.mkdirSync(env.UPLOADS_DIR, { recursive: true });

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

// Uploaded files have UUID names → URLs are effectively immutable, so the
// browser can cache them forever. nosniff still required defensively against
// any file that slipped past magic-byte validation.
app.use(
  "/uploads",
  (req, res, next) => {
    res.setHeader("X-Content-Type-Options", "nosniff");
    res.setHeader("Content-Disposition", "inline");
    next();
  },
  express.static(env.UPLOADS_DIR, {
    dotfiles: "deny",
    fallthrough: false,
    maxAge: "365d",
    immutable: true,
    setHeaders: (res) => {
      res.setHeader("X-Content-Type-Options", "nosniff");
      res.setHeader("Cache-Control", "public, max-age=31536000, immutable");
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

const { readDb } = require("./utils/db");

app.get("/server-status", async (req, res) => {
  try {
    const db = await readDb();
    const ok = Array.isArray(db?.users) && Array.isArray(db?.posts);
    if (!ok) throw new Error("db not seeded");
    res.status(200).json({
      status: "ok",
      message: "Server is running",
      users: db.users.length,
      posts: db.posts.length,
    });
  } catch {
    res.status(503).json({ status: "degraded", message: "DB unreadable" });
  }
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

app.use("/chat", (req, res, next) => {
  // Reads pass through; writes (POST/PATCH/DELETE) rate-limited.
  if (req.method === "GET") return next();
  return writeLimiter(req, res, next);
});

app.use("/users", userRoutes);
app.use("/posts", postRoutes);
app.use("/notifications", notificationRoutes);
app.use("/passkey", passkeyRoutes);
app.use("/chat", chatRoutes);

app.use((req, res) => {
  res.status(404).json({ message: "Route not found" });
});

// Centralized error handler — never leak `error.message`/stack to clients.
// eslint-disable-next-line no-unused-vars
app.use((err, req, res, _next) => {
  logger.error({ err, method: req.method, url: req.url }, "request error");
  // Multer file-size / fileFilter errors are user-facing
  if (err && err.code === "LIMIT_FILE_SIZE") {
    return res.status(413).json({ message: "File too large" });
  }
  if (err && err.message === "Unsupported file type") {
    return res.status(415).json({ message: "Unsupported file type" });
  }
  res.status(500).json({ message: "Something went wrong" });
});

const server = app.listen(PORT, async () => {
  // Eagerly warm the in-memory DB cache on boot so the first request is fast.
  try {
    await readDb();
  } catch {
    /* persisted on first write */
  }
  logger.info({ port: PORT }, "server up");
});

// Graceful shutdown: stop accepting new connections, give in-flight requests
// 10s to finish, then exit. Without this, restarts can drop pending writes.
const { close: closeDb } = require("./utils/db");
const shutdown = (sig) => () => {
  logger.info({ sig }, "shutting down");
  const timer = setTimeout(() => {
    logger.error("forced exit after 10s");
    process.exit(1);
  }, 10_000);
  timer.unref();
  server.close((err) => {
    if (err) {
      logger.error({ err }, "shutdown error");
      process.exit(1);
    }
    closeDb();
    logger.info("clean exit");
    process.exit(0);
  });
};
process.on("SIGTERM", shutdown("SIGTERM"));
process.on("SIGINT", shutdown("SIGINT"));
