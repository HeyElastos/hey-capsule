# AGENTS.md — Read this first

If you are an AI agent (Claude, Cursor, Copilot, anything else) — or a
new human contributor — landing in this repo, **read this page before
writing or proposing any code**. It will save you several days of
re-deriving things that have already been figured out.

## What this repo is

The HeyElastos capsule pack — every Hey-specific capsule, in one
repo, portable to any Elastos Runtime. Detailed overview in
[README.md](README.md).

## Where to read, in order

| # | Doc | When to read |
|---|---|---|
| 1 | [docs/runtime-quick-reference.md](docs/runtime-quick-reference.md) | **Always.** ~5 min. Key truths about the runtime that override most reasonable assumptions. |
| 2 | [docs/architecture-audit-2026-05.md](docs/architecture-audit-2026-05.md) | Before changing any cross-capsule behavior. ~10 min. The "why" behind the current code shape. |
| 3 | [docs/runtime-contract.md](docs/runtime-contract.md) | When you need source citations or details (provider bus shape, manifest schema, etc). ~50KB; navigate by section. |
| 4 | `capsules/<X>/STATUS.md` | Before touching any provider capsule. Each provider has a status doc explaining what works on stock upstream and what's draft / blocked. |
| 5 | `git log --since=2026-05-25 --pretty=full` | When you want the per-commit reasoning for recent changes. |

## Hard rules — don't violate these without checking

Most of these are corrections to things that LOOK reasonable but
aren't. Each links to where the truth is documented.

### Identity and auth

- **Don't store Ed25519 seeds or ML-KEM secrets in `localStorage`.** The
  capsule used to; it was wrong. The path forward is
  [identity-projection-provider](capsules/identity-projection-provider/)
  (currently draft). See [audit](docs/architecture-audit-2026-05.md).
- **Don't extract bearer tokens from launch-token redemption.** The
  runtime sets an HttpOnly cookie; `credentials: 'include'` on fetch
  carries it. No `Authorization: Bearer …` headers anywhere.
- **The runtime principal (`person:local:<hex>`) is NOT a social DID.**
  Don't display it as one; don't accept it where a `did:key:` is
  expected. See `inherit_session` for the correct probe order.
- **Launch token schema is `elastos.home.launch-token/v2`**, not v1.
  Signature domain is `elastos.home.launch.v1` (kept stable
  intentionally).

### Endpoints

- **`/api/apps/<id>/session/start` is NOT a generic per-app upstream
  route.** Upstream v0.3 routes it only for {documents, library,
  system, wallet, browser, chat-room}. The YNH fork's patch 0001
  adds hey-social and hey-messenger; on stock upstream this endpoint
  404s for our apps.
- **`/api/apps/<id>/runtime-token` does not exist in upstream at all.**
  We keep it as a fallback for older YNH builds; don't add new code
  that depends on it.
- **`/api/localhost/Users/*` returns 403 unconditionally** at the
  storage handler — the `Users/*` rejection runs BEFORE capability
  check. Principal-scoped storage goes through provider calls, not
  direct localhost URLs.
- **`provider_call("session", ...)` will always fail** today. The
  scheme is reserved but no built-in capsule implements it. Use
  `GET /api/session`.

### Provider proxy allowlist (the diagnostic you'll almost certainly hit)

- **A 403 on `/api/provider/<scheme>/<op>` calls is almost always the
  gateway provider proxy allowlist**, NOT a capability-token failure.
  Upstream v0.3's gateway hardcodes `allowed_apps = {documents,
  library, system, wallet, browser}` for the {peer, content, did,
  ipfs, ...} schemes. If your capsule isn't in that set, every
  provider call 403s **before the capability check even runs**.
  Source: `elastos/crates/elastos-server/src/api/gateway_provider_proxy.rs`
  (cited in [docs/runtime-contract.md](docs/runtime-contract.md)
  section A.2).
- **YNH fork's patch 0001 opens REDEMPTION only.** It adds hey-social
  + hey-messenger to the `/runtime-token` allowlist so we can mint a
  bearer/cookie. It does NOT add us to the provider proxy allowlist.
  So as of 2026-05-29 hey-social can authenticate but every
  Carrier/content/DID call still 403s.
- **There are exactly three options if you hit this:**
  1. Extend the fork patch (one diff: append hey-social + hey-messenger
     to the provider allowlist alongside the redemption opening).
     Rebuild runtime once.
  2. File an upstream PR to make the allowlist configurable. Same
     diff in `Elacity/elastos-runtime`; takes weeks but the fork's
     patch shrinks afterward.
  3. Accept that hey-social can authenticate but can't DO anything
     on this runtime — browse the feed, see empty everything.
- **You CANNOT bypass this from inside the Hey pack.** There's no
  client-side trick. The runtime is the gatekeeper. Anyone proposing
  an in-capsule workaround for a 401→403 pattern is wrong.

### Manifests

- **`capsule.json` is `#[serde(deny_unknown_fields)]`** — any typo or
  extra field fails install-time parsing.
- **`role: provider` REQUIRES `authority: { reason, capabilities[],
  audit_events[] }`**. Provider manifests without this block won't
  parse.
- **`permissions.messaging` and `permissions.storage` are INTENT, not
  authorization.** The capsule still has to call
  `/api/capability/request` at runtime; the runtime queues a pending
  request for shell approval. No auto-grant policy ships today.
- **Reserved scheme names are NOT shipped providers.** Per the
  upstream runtime audit, schemes like `peer`, `session`, `identity`,
  `principal`, `capabilities`, `message`, `storage` are in
  `RESERVED_SUB_NAMES` but have no built-in implementation. If you
  want one, you're greenfield (and probably need to patch the
  runtime registry).

### Federation and DMs

- **The runtime's `elastos://peer/*` is built into the runtime**, not a
  separate capsule. Don't build a "peer-provider"; one was tried at
  `76b7e58` and reverted at `1b49573` because the runtime
  short-circuits before any provider subprocess is spawned for this
  scheme.
- **Hey-social's DM design** uses random per-pair queue topics +
  sealed-sender envelopes. The runtime's `\x01DM:<pubkey>\x01<content>`
  marker convention is one valid DM pattern but NOT what Hey uses.
  Both are within spec ("DMs are an application-layer convention").

### Build hygiene

- **`dist/` is committed and CI-verified.** Any change to capsule
  source MUST come with a rebuilt `dist/`. The
  [verify-dist.yml](.github/workflows/verify-dist.yml) workflow fails
  the PR if they don't match.
- **Capsule wasm is built with `trunk build --release`** in the
  capsule's own directory (`capsules/hey-social/`), not from the
  workspace root. Trunk handles content-hashing the wasm filename.
- **Don't auto-`git push`.** The user pushes deliberately. Commits
  locally are fine; pushing requires explicit "push" from the user.
  See `[Local-first workflow]` in any agent's memory or the team's
  conventions.

### Code style

- **Don't write code comments that explain WHAT the code does** —
  good names do that. Only comment the WHY (hidden constraint, subtle
  invariant, source of truth in another file).
- **Don't reference current task / PR numbers in comments.** Those
  belong in the commit message, not the code.
- **Don't add backwards-compat shims you don't need.** If you're
  certain something is unused, delete it.

## Specific gotchas

- **The capsule's `runtime.rs` is the single boundary against the
  runtime.** Everything else (`events.rs`, `pages/*`, `components/*`,
  `api/*`) calls helpers there. When the runtime contract changes,
  only `runtime.rs` should need to change.
- **Trunk packaging is racy on first invocation** — if you see `error
  writing JS loader file to stage dir / No such file or directory`,
  just rerun `trunk build --release`. The second run succeeds.
- **`hey-social-rust` → `hey-social`.** The capsule was renamed
  2026-05-28 (commit `eec5390`). Any old reference to
  `capsules/hey-social-rust/` should be `capsules/hey-social/`.
- **The reference React `hey-social` is gone.** Hey-social is now
  Rust + Leptos + WASM. Don't propose React-style fixes; we're
  Rust-side.
- **The `peer` provider uses iroh-gossip internally.** Wire-format
  audit in [docs/runtime-contract.md](docs/runtime-contract.md)
  section A. Recv response field is `content`, not `message`; we
  read both for back-compat.

## Where to drill further

- **Upstream runtime source**: https://github.com/Elacity/elastos-runtime
  (audit was against commit `6d4c385`, 2026-05-28)
- **YNH packaging fork**: https://github.com/HeyElastos/elastos-runtime_ynh
  — especially `scripts/patches/`, `components.additions.json`,
  `scripts/_common.sh`
- **Canonical reference impls** (for "how do I do X correctly?"):
  - Free-form provider: upstream `capsules/did-provider/src/main.rs`
  - Storage-shaped provider: upstream `elastos/capsules/localhost-provider/src/main.rs`
  - Cookie-based session-start client: upstream `capsules/chat-room/browser/index.html`
- **In-pack examples**:
  - This pack's `capsules/blobs-provider/src/main.rs` — small, working,
    canonical Rust provider with stdin/stdout JSON.

## When to ask the human vs. proceed

- Ask before: pushing to origin, building anything that requires a
  scheme not in this list of decisions, touching the runtime repo,
  files outside this repo's `capsules/` + `docs/` + `.github/`.
- Just do it: typo fixes, in-capsule refactors, doc updates,
  adding tests, rebuilding dist after a source change you already
  agreed to, following an explicit user instruction.

## Conventions for editing this file

- Date stamp any new "hard rule" with the commit it landed in.
- When the runtime evolves past a documented gotcha, **delete the
  rule**, don't leave it with a "see also". Stale rules in this file
  are worse than missing ones.
- If you add a new audit doc, link it in the table at the top.
