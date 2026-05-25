# Hey — Modernization Summary

## Overview

**Status**: Shipped
**Last updated**: 2026-05-25
**Scope**: Full rewrite of the original SocialEcho codebase into **Hey**, an
image-first, key-based social app for Elastos.

The repo directory is still named `SocialEcho` for historical reasons; the
product is **Hey**.

---

## What changed

The original SocialEcho stack (community-centric Facebook clone, MongoDB,
email/password auth, classifier microservice, admin panel, moderator
tooling) was replaced with a Pixelfed-style photo/video social network.

### Removed
- MongoDB + Mongoose models (24+ model files)
- Email/password auth, verification flow, suspicious-login detection
- Communities, moderators, admin panel, reports, rules
- Python classifier microservice (`classifier_server/`)
- Redux store, actions, reducers, constants (the full `redux/` tree)
- Old shared layout (Navbar, Leftbar, Rightbar, Search)

### Added
- File-based JSON database (`server/data/db.json`) with in-memory cache,
  atomic temp-file writes, and serialized writes via `async-mutex`
- Key-based identity (generated nickname + secret key, no email)
- WebAuthn / passkey auth (Yubikey, Nitrokey, Touch ID, Windows Hello)
- AVIF transcoding pipeline (`sharp`) for uploaded photos
- Magic-byte validated video uploads with immutable cache headers
- Two-environment UX: photo feed at `/`, video feed at `/videos`
- Threaded comments with per-comment reactions and hover-to-hide
- Onboarding flow with profile setup and celebration
- QR-code profile share
- Hardening pass: Helmet, locked-down CORS, rate limits, JWT auto-refresh

---

## Current architecture

### Frontend (`client/`)

Vite + React 18 + Tailwind. Dev server on port 3000, proxies `/api` and
`/uploads` to the backend.

**Pages** ([client/src/pages/](client/src/pages/))
- `Landing.jsx` — unauthenticated entry
- `SignUp.jsx` — key generation + optional passkey enrollment
- `Onboarding.jsx` — profile setup with handwritten welcome
- `Home.jsx` — photo feed
- `Clips.jsx` — video feed
- `Posts.jsx` — create-post composer
- `PostDetail.jsx` — single-post view with comments
- `Profile.jsx` — own profile, photo grid + video grid
- `VideoPlayer.jsx` — full-screen vertical video player

**Components** ([client/src/components/](client/src/components/))
- `FloatingDock.jsx` — primary nav, env switcher (camera ↔ video)
- `PostCard.jsx`, `ImageCarousel.jsx`, `HeyVideoPlayer.jsx`, `SafeMedia.jsx`
- `ImageDropzone.jsx` — iPhone-style multi-photo upload preview
- `CommentBubble.jsx`, `ReactionPicker.jsx`
- `SignInModal.jsx`, `PasskeyManagerModal.jsx`, `PasskeyStatusModal.jsx`
- `ProfileEditModal.jsx`, `DeleteAccountModal.jsx`, `SearchModal.jsx`
- `NotificationPanel.jsx`, `QRBadge.jsx`, `icons.jsx`

**API** ([client/src/api/](client/src/api/))
- `auth.js` — axios instance with JWT auto-refresh on 401
- `passkey.js` — WebAuthn ceremony helpers

### Backend (`server/`)

Node + Express on port 4000. No external database — single-file JSON store.

**Controllers** ([server/controllers/](server/controllers/))
- `user.controller.js` — signup/signin, profile, follow
- `post.controller.js` — create/read posts, comments, reactions
- `notification.controller.js` — notification feed
- `passkey.controller.js` — WebAuthn registration + assertion

**Middlewares**
- `auth.js` — required JWT
- `optionalAuth.js` — soft-auth for public reads

**Utils**
- `db.js` — JSON store with mutex-serialized atomic writes (+ `db.test.js`)
- `secrets.js` — JWT secrets (env in prod, persisted dev secret)
- `env.js` — env validation (refuses to start in prod without secrets)
- `video.js` — magic-byte validation + transcoding helpers
- `notifications.js`, `logger.js`

---

## Security posture

- **Auth**: JWT access tokens (6h) + refresh tokens (7d), per-process
  signing keys. Refresh secret persisted to `server/data/.secrets.json` in
  dev; required env vars in production.
- **Transport**: Helmet, locked-down CORS allowlist (`CLIENT_ORIGIN`).
- **Rate limiting**: separate limiters for auth, writes, and uploads.
- **Upload safety**: magic-byte validation for both images and videos.
  Images auto-transcoded to AVIF. `/uploads` served with
  `Content-Disposition: inline` + `X-Content-Type-Options: nosniff`.
- **WebAuthn**: `RP_ID` + `WEBAUTHN_ORIGIN` env vars in prod.

---

## Recent commits

| Commit | Summary |
|--------|---------|
| `457f72a` | Frontend robustness + license/docs cleanup (removed stale `QUICK_START.md`, `FRONTEND_INTEGRATION_GUIDE.md`, `FRONTEND_SETUP_CHECKLIST.md`, `COMPONENT_REFERENCE.md`) |
| `ff2f29e` | Backend hardening: SQLite-style JSON store with mutex, transcoding, validation, structured logging |
| `c42769e` | Security hardening across server + uploads + auth; added passkey controller + modals |
| `7be7772` | Rewrite as Hey: Pixelfed-style photo/video social on Elastos |

---

## Running

```bash
# Backend
cd server
npm install
npm start         # listens on :4000

# Frontend (in another shell)
cd client
npm install
npm run dev       # listens on :3000
```

Open <http://localhost:3000>. Pick a nickname, copy the generated key,
you're in.

### Production env

```bash
SECRET=<random 32+ chars>
REFRESH_SECRET=<random 32+ chars>
CLIENT_ORIGIN=https://your.domain
RP_ID=your.domain
WEBAUTHN_ORIGIN=https://your.domain
NODE_ENV=production
```

The server refuses to start in production without `SECRET` and
`REFRESH_SECRET`.

---

## Known gaps / future work

- No automated end-to-end tests; backend has unit coverage on `db.js` only.
- Notifications are pull-based; no websocket/push delivery yet.
- Single-node JSON store — not horizontally scalable; migrate to SQLite or
  Postgres before deploying beyond a single instance.
- No image moderation or NSFW detection (the original classifier service
  was removed).
- Video thumbnails are not generated server-side.

---

## See also

- [README.md](README.md) — user-facing setup and feature overview
- [LICENSE](LICENSE) — MIT
