# hey-core migration audit & plan

**Goal:** hey-core is the shared platform/SDK; apps (hey-social, hey-messenger,
future hey-mail) are thin shells. Write an infra capability **once** in
hey-core and every app inherits it. Rule of thumb: **infra → hey-core;
domain + UI → stays per-app.** Never break a running app — `cargo check
--target wasm32-unknown-unknown` (and `trunk build`) green after every module.

Status legend: ✅ done · 🔜 next · ⏳ later · 🧊 frozen (do not touch)

---

## Module classification (hey-social vs hey-core engine)

### ✅ Done — re-exported from engine (committed)
| Module | Note |
|---|---|
| `crypto.rs` | `pub use hey_core::crypto::*`. Fixed real bug: social was on `hpq-1`, engine on `hpq-2`; couldn't interop. Engine is backward-compatible superset (key derivation byte-identical, decrypts `hpq-1`). |
| `identity.rs` | `pub use hey_core::identity::*` (was byte-identical leaf). |
| `session.rs` | `pub use hey_core::session::*`. `Session` struct identical; keys come from `CapsuleCtx` (set in `main.rs`). Same localStorage keys → no re-login. |
| `RuntimeError` | `pub use hey_core::runtime::RuntimeError` (struct+impls were identical, no extra impls). Unifies the error type across apps — the keystone that lets engine-backed modules return errors hey-social handles directly. |
| `passkey.rs` | `pub use hey_core::passkey::*` (was byte-identical; was blocked only on the `RuntimeError` unification above). Engine version runs under `CapsuleCtx` → same `/api/apps/hey-social/*` calls + same shared-identity dual-write. |

`CapsuleCtx::init(...)` is wired in `hey-social/src/main.rs` with hey-social's
exact values (capsule_id "hey-social", namespace "Hey", session_key
"hey-social-session", + all sessionStorage keys + the 5-entry boot wants-list).

### 🔜 Phase A.2 — runtime auth/transport core + passkey (verified equivalent, NOT yet executed)
These are **equivalent modulo ctx** in both files (diffed): `RuntimeError`,
`api_base`, `api_url`, `home_launch_token`, `redeem_launch_token`,
`bearer_ready`, `upstream_fetch`, `ensure_capability_token`, `provider_call`,
`peer`, `transcoder`, `storage`, `identity_provider` (API matches),
`shared_read/write_json`, `acquire_boot_capabilities`,
`scrub_launch_token_from_url`, `inherit_session`, `session_current`,
`SharedIdentity`. **This is hey-social's fixed home-passkey auth** — verified
the engine carries it (commits be2d75a / 7807116 / 330f13e).

- `passkey.rs` is **byte-identical** but **BLOCKED**: it returns `RuntimeError`,
  so it can only be re-exported once `crate::runtime::RuntimeError` IS the
  engine's (i.e. after the runtime re-export). Migrate runtime → then passkey.
- **Execution caution:** `runtime.rs` interleaves the safe auth core with
  social-ahead modules (below) and private helpers (`fetch_raw`, `log_warn`,
  `window`, `read_url_token`). The re-export must be **selective** (can't glob —
  the kept social modules would name-clash). Read the whole file first; keep
  the private helpers the kept modules use.

### ⏳ Social-AHEAD — must promote into engine OR keep local (never naive re-export)
| Module | Why |
|---|---|
| `runtime::content` / `ipfs` | social has the **CID byte cache** (this session); engine lacks it. |
| `runtime::did_provider` | social has ~100 lines vs engine ~20 — social ahead. |
| `events.rs` | social has **provider-backed signing** (M3); engine's is older. |
| `api/profile.rs` | social 268 lines vs engine 32-line stub — social ahead. |

### ⏳ Engine-AHEAD — social would UPGRADE by adopting (chat infra)
| Module | Why |
|---|---|
| `api/dms.rs` | engine has **Double Ratchet M6** (2607 lines) vs social's 1337. |
| `api/outbox.rs` | **identical** (0 diff) — safe re-export anytime. |

### ⏳ Needs engine enhancement
| Module | Work |
|---|---|
| `peer_receiver.rs` | engine handles DM routing only; social needs posts/follows. Make engine `route()` **pluggable** (engine already left a note for this), then each app registers its own handlers. |

### 🟢 Social-ONLY — stays in the app forever (domain + UI)
`ipld.rs` (post-body schema — but the *generic dag-cbor codec* can move to
engine), `api/posts.rs`, `api/groups.rs`, `api/notifications.rs`, `pages/`,
`components/`, `app_modals.rs`, `lib.rs`, and the boot-splash helpers
`boot_log` / `warp_boot_into_feed` / `hide_boot_splash` / `sleep_ms`.

---

## 🧊 Frozen constants — DO NOT rename/change
- `b"hey-chat/ratchet/root-init/v1"`, `.../root/v1`, `.../mk/v1`, `.../ck/v1`
  (Double Ratchet HKDF domain separation, `hey-core/src/crypto.rs`).
- `HKDF_INFO = b"hey-messenger/hpq-1"` (stable across hpq-1→hpq-2).
- Per-capsule session/storage keys stay **distinct** per app (independent
  per-app sign-in, same DID). That's intended, not drift.

---

## Phases
- **Phase 0 ✅** rename hey-chat→hey-core; dedupe crypto + identity. (committed)
- **Phase A.1 ✅** `CapsuleCtx` wiring + `session` re-export. (this step)
- **Phase A.2 🔜** runtime auth/transport selective re-export + `RuntimeError`
  unification + `passkey` re-export. Keep social-ahead modules (`content`,
  `did_provider`) + boot helpers local. Also re-export `api/outbox` (identical).
- **Phase B ⏳** promote `content`+CID cache and reconcile `did_provider` into
  the engine; extract generic dag-cbor codec → engine. Then social re-exports.
- **Phase C ⏳** pluggable `peer_receiver::route()`; port `events`
  provider-signing into engine; adopt engine `api/dms` (Double Ratchet) for
  social; reconcile `api/profile`.
- **Forever:** domain + UI stay per-app. A future **hey-mail** is then a thin
  shell: hey-core (auth/crypto/identity/IPFS/CID/IPLD/transport) + mail domain.

## No-break rules
1. `cargo check --target wasm32-unknown-unknown` after every module change.
2. Never re-export a **social-ahead** module (regression). Promote it up first.
3. `RuntimeError` must be the engine's (re-export) before anything returning it
   (`passkey`, etc.) can be re-exported.
4. `CapsuleCtx::init` runs first in `main()`, before any engine-backed call.
