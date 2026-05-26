# Hey Social

A photo and video social app on Elastos. Image-first, key-based identity,
no email, no algorithm.

## What it is

Hey Social lets people share photos and short videos with friends, react
with any emoji, comment, reply, and repost. You sign in with a private key
(or a hardware passkey — Yubikey, Nitrokey, Touch ID, Windows Hello). No email,
no password, no ads.

## Highlights

- **Two feeds, one app**: Photos at `/` and videos at `/videos`. The top
  bar's camera / video icons switch environments; the active environment is
  marked with an accent-tinted pill and a glowing underline.
- **Key-based signup** — generate a nickname, get a secret key, that's your
  identity. Keep it safe.
- **Passkey support** — optional WebAuthn (FIDO2). Sign up or sign in by
  touching a hardware key or using your platform biometric. Manage multiple
  passkeys from your profile.
- **Image pipeline** — uploads are auto-converted to AVIF (high quality,
  small files) on the server via `sharp`. Max 12 images per post.
- **Video pipeline** — direct upload, magic-byte validated, served with
  immutable cache headers.
- **Comments with threading** — reply to comments, react to comments,
  hide individual comments on hover, collapse the whole section.
- **iPhone-style upload preview** — multi-photo posts stack like the
  Photos app, with a thumbnail strip to reorder.
- **Profile** — handwritten brand mark, click-to-upload avatar, QR-code
  share, photo grid + video grid split by mode.
- **Onboarding** — handwritten welcome, profile setup, party-popper
  celebration before landing in your feed.

## Stack

- **Frontend** — Vite + React 18 + Tailwind, served on port 3000. Routes,
  modals, and animations live in `client/src`. The dev server proxies
  `/api` and `/uploads` to the backend.
- **Backend** — Node + Express, served on port 4000. File-based JSON
  database at `server/data/db.json` with in-memory caching, atomic
  temp-file writes, and serialized writes via `async-mutex`.
- **Auth** — JWT access tokens (6h) + refresh tokens (7d) signed with
  per-process secrets (persisted to `server/data/.secrets.json` in dev,
  required env vars in prod). Auto-refresh on 401 via an axios interceptor.
- **Security** — Helmet, locked-down CORS, rate limits on auth + writes +
  uploads, magic-byte file validation, `Content-Disposition: inline` +
  `nosniff` on `/uploads`.

## Getting started

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

Open <http://localhost:3000>. Pick a nickname, copy the generated key, and
you're in.

### Optional env vars (production)

```bash
SECRET=<random 32+ chars>           # JWT signing key
REFRESH_SECRET=<random 32+ chars>   # JWT refresh signing key
CLIENT_ORIGIN=https://your.domain   # CORS allowlist
RP_ID=your.domain                   # WebAuthn relying-party id
WEBAUTHN_ORIGIN=https://your.domain # WebAuthn expected origin
NODE_ENV=production                 # refuses to start without secrets
```

## Project structure

```
client/src/
  api/                axios helpers (auth, passkey)
  components/         PostCard, ImageCarousel, modals, FloatingDock, …
  pages/              Home, Clips, Profile, VideoPlayer, Onboarding, Landing
server/
  app.js              express setup, helmet, rate limits, error handler
  controllers/        user, post, notification, passkey
  middlewares/        auth, optionalAuth
  routes/             route definitions
  utils/              db, secrets, notifications
  data/               db.json (created on first run)
  uploads/            user-uploaded media (gitignored)
```

## License

MIT
