# Hey Social

A photo, video, and chat social app on Elastos. Capsule-native,
federated peer-to-peer over Elastos Carrier, media on IPFS, sovereign
identity. No backend, no email, no algorithm.

## What it is

Hey Social lets people share photos, short videos, and chat with friends,
react with any emoji, comment, reply, and repost. You sign in with a
recovery key (or a hardware passkey — Yubikey, Nitrokey, Touch ID,
Windows Hello). No email, no password, no ads.

It runs as an Elastos Runtime capsule: a sandboxed microvm that talks
only to the runtime's storage, Carrier, IPFS, and DID providers. No
Hey-owned server runs anywhere. Two phones on the same WiFi can
federate over LAN with no internet (Carrier and IPFS both have mDNS);
cross-internet federation uses Carrier's DHT + relay infrastructure.

## Highlights

- **Two feeds + chat, one app**: photos at `/`, videos at `/videos`,
  DMs and group rooms at `/chat`.
- **Sovereign identity** — Ed25519 keypair derived from a recovery key
  you generate locally. Same identity across all your devices via the
  shared profile contract; one DID across Hey and the desktop shell.
- **Hardened signing** — your private key is imported as a NON-EXTRACTABLE
  Web Crypto CryptoKey and persisted in IndexedDB. The raw seed never
  appears in JS memory or storage after import. XSS cannot exfiltrate
  the key.
- **Passkey support** — optional WebAuthn (FIDO2). Sign up or sign in by
  touching a hardware key or using your platform biometric. Manage
  multiple passkeys from your profile.
- **Media pipeline** — photos, videos, and voice clips run through the
  `hey-transcoder` capsule (WebP @ 2048px / H.264 @ 1080p CRF 23 /
  Opus @ 64 kbps LUFS-normalized), then pinned to IPFS via the
  `ipfs-provider` capsule. Content-addressed, dedup'd, replicated.
- **Federation over Carrier** — every post, comment, reaction, DM,
  voice message, room message is a signed gossip event published to
  a Carrier topic. Peers verify signatures on receive.
- **Comments with threading** — reply, react, hide-on-hover, collapse.
- **iPhone-style upload preview** — multi-photo posts stack like the
  Photos app, with a thumbnail strip to reorder.
- **Profile** — handwritten Dancing Script brand mark, click-to-upload
  avatar, QR-code share, photo + video grids.
- **CSP-hardened** — strict Content-Security-Policy in `index.html`
  (`script-src 'self'`, `object-src 'none'`, etc.) blocks injected
  `<script>` tags.

## Stack

- **Runtime container** — Elastos Runtime microvm, declared as
  `elastos.capsule/v1`. See the [hey-capsule](https://github.com/HeyElastos/hey-capsule)
  repo for the packaging (busybox + Node + Hey app + `init` baked into
  `rootfs.ext4`).
- **Frontend** — Vite + React 18 + Tailwind. The compiled bundle is what
  ships inside the microvm.
- **Talking to the host** — every fetch goes through the runtime HTTP
  surface:
  - `/api/localhost/Users/self/.AppData/LocalHost/Hey/*` (storage)
  - `/api/localhost/Users/self/.AppData/Identity/profile.json` (shared identity)
  - `/api/provider/peer/*` (Carrier gossip)
  - `/api/provider/ipfs/*` (IPFS via Kubo)
  - `/api/provider/did/*` (DID resolution)
  - `/api/provider/hey-transcoder/*` (ffmpeg / WebP / Opus)
- **Identity & signing** — Ed25519 (`@noble/curves` for derivation,
  Web Crypto for the persisted non-extractable signing key).
- **Required capsules** — `ipfs-provider` (declared in
  [hey-capsule/capsule.json](https://github.com/HeyElastos/hey-capsule)),
  optional `hey-transcoder` for normalized media.

## Running

Hey Social is built and shipped as part of the
[hey-capsule](https://github.com/HeyElastos/hey-capsule) packaging.
Install on an Elastos Runtime that has `ipfs-provider` available; launch
from the shell (hey-home or the stock home shell).

To work on Hey locally with hot reload pointed at a runtime gateway:

```bash
cd client
npm install
npm run dev
```

The Vite dev server proxies `/api/*` to a configured runtime gateway
(see `vite.config.js`). Open <http://localhost:3000>, pick a nickname,
copy the generated recovery key, you're in.

## Project structure

```
client/
  index.html            CSP meta, favicon link
  public/hey-icon.svg   white Dancing Script "hey" wordmark
  src/
    api/                capsule API: auth, chat, passkey
    lib/                runtime client, identity, keystore, session,
                        events, shell-bridge
    components/         PostCard, ImageCarousel, FloatingDock, modals, …
    pages/              Home, Clips, Profile, Chat, VideoPlayer,
                        Onboarding, Landing
    main.jsx            boots initSession() then mounts the app
```

## License

MIT
