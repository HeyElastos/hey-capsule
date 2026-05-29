# Architecture Audit — 2026-05

> Snapshot of the architectural decisions, dev framing, and concrete
> corrections that drove the work landed in commits `d0507f1` →
> `0f80f60` (2026-05-28). Read this alongside
> [runtime-quick-reference.md](runtime-quick-reference.md) to
> understand WHY the code looks the way it does today.

## The reset moment

The pack's app capsule used to be a React SPA (`capsules/hey-social/`)
that talked to the runtime through patches and assumptions that
diverged from the upstream contract. A dev review surfaced:

1. **Too much capsule-side identity work.** Hey ran its own passkey
   flow, derived its own `did:key:`, stored the Ed25519 seed and
   ML-KEM secret in `localStorage`. Any XSS in any bundled dep read
   the credentials.
2. **Bearer-token auth** instead of cookie auth. Hey extracted a
   bearer from `/runtime-token` and stamped `Authorization: Bearer …`
   on every fetch. Both the credential and the convention were
   capsule-managed.
3. **Direct IPFS access** (`ipfs.add_bytes`), assuming the capsule
   had ambient permission to talk to system providers.
4. **Reads from shared identity paths** (`.AppData/ElastOS/Identity/*`,
   `.AppData/Identity/*`) — outside the capsule's grant; the runtime
   was correct to reject them.
5. **Confused `principal` with social `did:key:`**. `person:local:…`
   showed up as the user's DID in the profile UI — wrong ontology.
6. **No federation actually happening.** The "peer provider" the chat
   code was calling didn't exist on the box; every `gossip_send` was
   silently dropped. Local writes made the UI look like it worked.

Dev framing in two sentences:

> The right fix is more providers, not more permissions. Strip the
> ambient access; build small focused providers; let the capsule
> ask each one for the surface it needs.

## What landed (capsule code)

### Hey-social cleanup (`d0507f1`, `0b8ac35`, `1b49573`, `be2d75a`)

- **`capsule.json` storage scope reduced to one entry:**
  `localhost://Users/self/.AppData/LocalHost/Hey/*`. Every other
  shared-identity / shell scope removed.
- **`capsule.json` messaging declares INTENT only** — `peer`, `content`,
  `identity`, `social-feed`, `did`, `hey-transcoder`, `elacity`. These
  are manifest-declared scopes the capsule may request at runtime;
  they are NOT automatic grants (per
  [runtime-quick-reference.md](runtime-quick-reference.md): no
  auto-grant policy ships today).
- **Shared-identity reads + dual-write deleted.**
  `.AppData/ElastOS/Identity/*` and `.AppData/Identity/*` paths are
  gone. `src/shell.rs` deleted. `ensure_profile` synthesizes from
  `session.did_key` only — the PRF-derived social DID.
- **PUT 412 (create-only conflict) downgraded to silent success.** The
  feed-index append pattern legitimately tries to overwrite a
  create-only file; treating that as a hard error spammed the UI.
- **First-run profile GET 404 is silent.** `storage::read_json`
  returns `Ok(None)` on 404, no log; `ensure_profile` writes the
  initial profile, future reads succeed.
- **Launch-token contract switched to cookie auth.**
  `redeem_launch_token` (renamed from `bearer_ready`) POSTs to
  `/api/apps/<id>/session/start` first, falls back to
  `/runtime-token` for older builds. Either response sets an HttpOnly
  cookie; the capsule no longer holds a bearer. Every
  `Authorization: Bearer …` injection site removed.
- **`inherit_session` DID-only filter.** Probe order: `didKey`,
  `did_key`, `did`, and nested `user./identity.` variants. `principal`
  intentionally excluded — even if a future principal happened to
  start with `did:`, it would still be the runtime principal, not the
  social DID.
- **Peer wire-shape compliance.** `peer_receiver` reads `content`
  field first, falls back to `message` (legacy). Per-pair queue
  topics + sealed-sender envelope kept (the spec explicitly allows
  this DM convention).

### New provider drafts (`c0353c7`, `959554d`, `0f80f60`)

#### [identity-projection-provider](../capsules/identity-projection-provider/)

Answers `elastos://identity/*` with `whoami / sign / verify`. Holds
the Ed25519 seed; capsules never see the secret. HKDF-derived
per-namespace keys for cross-capsule continuity.

**Status: draft.** `identity` is NOT in the runtime's
`RESERVED_SUB_NAMES` (see [STATUS.md](../capsules/identity-projection-provider/STATUS.md)
in the provider). To actually dispatch: patch the runtime registry,
rename to a non-reserved scheme, or use the YNH-fork patch path.

#### [content-provider](../capsules/content-provider/)

Answers `elastos://content/*` with `publish / fetch / ensure /
unpublish` on top of kubo. Maps policy hints ("network_default",
"local_pin", "transient") to pin lifecycle.

**Status: draft.** Upstream implements `elastos://content/*` as
`crate::content` (server-side, not a separate capsule). Installing
this on stock upstream is a no-op — the runtime short-circuits before
the provider registry is consulted. See
[STATUS.md](../capsules/content-provider/STATUS.md) for the three
options to actually wire dispatch.

### Build hygiene (`0b4a0db`)

[.github/workflows/verify-dist.yml](../.github/workflows/verify-dist.yml)
— on every push/PR rebuilds hey-social from a clean state and
compares the dist/ tree hash. The committed bundle must match a
clean rebuild from the same commit. Also builds every provider. The
WASM ↔ commit relationship is now reviewable.

## What was reverted

### peer-provider capsule (`76b7e58`, reverted in `1b49573`)

I built a standalone iroh-gossip capsule answering `elastos://peer/*`.
**It's redundant** — the runtime already provides this surface via
its built-in iroh stack. Capsules call `provider_call("peer", ...)`
and the runtime dispatches internally. The smoke test of my
peer-provider passed in isolation but the binary would never have
received a request on a real install.

Lesson: check `RESERVED_SUB_NAMES` (registry.rs:163) before building
a provider. Reserved schemes may already have built-in handlers.

## Open architectural decisions

These are unresolved as of 2026-05-28 and will need a call before
more code lands:

1. **What namespace does identity-projection-provider live under?**
   Three options in its STATUS.md. Picking one unblocks the
   capsule-to-provider migration.

2. **Does content-provider replace `crate::content` or sit beside it?**
   The dev framing wants a single content surface with transcode
   policy + dDRM. Server-side `crate::content` is a thin pass-through
   to kubo. Either we patch the runtime to delegate, or we accept
   the duplicate and let hey-social use whichever is dispatched.

3. **When do we file upstream PRs?**
   The YNH fork's patch 0001 adds hey-social/hey-messenger to the
   `/session/start` allowlist. A planned patch revision adds
   generic `/session/start` + OPTIONS support. Once stable, file
   upstream to make the allowlist configurable so we don't
   indefinitely fork.

4. **Hey-social keystore migration.**
   The Ed25519 seed + ML-KEM secret still live in localStorage. The
   identity-projection-provider exists with the right contract, but
   hey-social hasn't swapped its in-bundle derivation for
   `identity.sign` RPCs yet. Mechanical change once we resolve #1.

5. **DM routing scheme.**
   Hey-social uses per-pair random queue topics + sealed-sender
   envelopes. The dev's reference flow (chat-room) doesn't use the
   `\x01DM:` marker convention either, but uses runtime room-service
   objects, not gossip. Decide whether hey-social stays on
   gossip-with-queues or migrates to a future room-service-like
   surface.

## Pointers for the next agent

- [docs/runtime-quick-reference.md](runtime-quick-reference.md) — key
  truths first
- [docs/runtime-contract.md](runtime-contract.md) — full audit with
  source citations
- [`capsules/*/STATUS.md`](../capsules/) — per-provider status
- `git log --since=2026-05-25 --pretty=full` — commit messages carry
  the per-change reasoning
- [HeyElastos/elastos-runtime_ynh](https://github.com/HeyElastos/elastos-runtime_ynh)
  → `scripts/patches/` — the YNH-side patches against upstream

## Addendum: dev confirmation 2026-05-29

After the audit shipped, the dev independently verified the
diagnosis and added three corrections that tighten what we knew:

### 1. The provider proxy allowlist is even narrower

What the screenshot suggested: `{documents, library, system, wallet,
browser}`. What `gateway.rs:1156` actually does in the dev's
checkout: it only accepts `documents`, and only allows `documents`
and sometimes `library` launch tokens through. So the locked-out set
is bigger than I'd documented; the unlocked set is smaller. Either
way the diagnosis is correct: Hey can authenticate but cannot DO
anything until the allowlist is changed.

### 2. `/api/session` does not return identity at all

Confirmed: the response is `SessionInfoOutput { session_id,
session_type, vm_id, capabilities_count, created_at, last_active }`
(`handlers/capability.rs:527-572`). No `did`, no `principal`, no
identity claim. Our `inherit_session` probe is correct to look for
identity fields defensively, but it WILL return `None` on stock
upstream — that's expected behavior, not a bug. Landing falls back
to the passkey ceremony, which is the "transitional empty
auth_key_hex + lazy passkey" path the dev framing endorses.

### 3. The proper fix is manifest/capability-based, not allowlist-extension

The dev pushed back on the short-term option I documented (extend
the YNH fork patch to add hey-social + hey-messenger to the
hardcoded allowlist). That fix unblocks today but adds more
hardcoded-app-name sprawl — explicitly NOT runtime-aligned.

The runtime-aligned model:

1. Installed capsule manifest declares required capabilities
2. Home launch token binds `{app + capsule_hash + session}`
3. `/runtime-token` (or `/session/start`) creates app-scoped session
4. App requests declared capabilities
5. Home/System approves (or trusted policy auto-grants)
6. Provider proxy validates the **capability token**, not the
   hardcoded app name

Upstream PR direction: make the allowlist configurable so it checks
capability tokens. The fork patch shrinks once this lands; we stop
forking the runtime for app-name reasons.

### 4. Ed25519 in browser/app state is a transitional unblock

The dev confirmed: localStorage-resident Ed25519 + ML-KEM is
acceptable for getting hey-social running today, but is NOT
runtime-aligned. The real fix is Runtime-owned signing through
`elastos://did/sign` or an app-scoped identity provider — capsule
asks for a signature, never holds the key.

The
[identity-projection-provider](../capsules/identity-projection-provider/)
in this pack has the right wire shape. It's blocked on scheme
dispatch (the `identity` scheme isn't in `RESERVED_SUB_NAMES`; the
runtime registry would need either an upstream patch or a YNH-fork
patch to route to our binary). Until then, localStorage stays — but
treat it as known XSS surface, not architecturally correct.

### Net status after this confirmation

| Concern | State |
|---|---|
| Audit's structural diagnosis | ✅ confirmed by dev independently |
| Right fix direction | manifest/capability-based, not allowlist sprawl |
| Identity inheritance from /api/session | Won't work, by design — and that's OK |
| Ed25519 in localStorage | Tolerated as transitional; not a long-term answer |
| In-capsule workaround for 401→403 | Confirmed impossible — runtime is the gate |

## YNH-fork patch lifecycle (designed for removal)

The YNH fork at
[HeyElastos/elastos-runtime_ynh](https://github.com/HeyElastos/elastos-runtime_ynh)
carries patches in `scripts/patches/`. Each is **scaffolding** —
it bridges a gap between stock upstream and what Hey needs, and is
meant to disappear when upstream merges the equivalent fix. Two
distinct fix shapes can close the gap; both result in the same
removal procedure.

### Removal cases

**Case A — upstream merges the proper fix (capability-token validation).**
The hardcoded allowlist disappears from upstream entirely; the
provider proxy now validates `X-Capability-Token` instead. Our
patch tries to modify a `match` block that no longer exists →
`scripts/_common.sh` halts the next install with a patch-apply
error. Loud signal. Delete the patch file, bump
`UPSTREAM_VERSION` to the release that includes the fix, re-test,
ship.

**Case B — upstream merges the short fix (just adds Hey to the
hardcoded list).** Our patch becomes a duplicate of what's in
upstream. Same removal procedure: delete the patch file, bump
`UPSTREAM_VERSION`, re-test, ship.

Either way the capsule code requires no changes — it's already
spec-compliant for both worlds.

### Discipline that keeps patches from outliving their purpose

1. **Every patch file MUST carry a kill condition in its header.**
   Format:
   ```
   # Target: <upstream file path>
   # Generated against: Elacity/elastos-runtime @ <commit>
   # Kill condition: DELETE this file when <upstream PR # | release
   #   tag> merges/ships.
   # Why: <one paragraph linking the gate this opens to the capsule
   #   behavior it unblocks>
   ```
   No header → reject on review.

2. **File the upstream PR before or alongside landing any new
   patch.** Reference the PR number in the patch header. Without a
   visible cross-link, the patch outlives its purpose.

3. **`scripts/_common.sh` halts the install on patch-apply
   failure.** That's a feature: when upstream changes the file we
   patched, install fails loudly → we delete or regenerate. Loud
   beats silent drift.

4. **On every `UPSTREAM_VERSION` bump:** re-test every patch.
   Either it still applies (keep it), the upstream PR landed
   (delete it), or upstream diverged (regenerate against new
   upstream and refresh the header).

### Currently in flight (as of 2026-05-29)

- **`0001-allow-hey-redemption.patch`** — opens `/runtime-token`
  for hey-social + hey-messenger. Already landed.
- **`0002-allow-hey-provider-access.patch`** *(planned)* — would
  extend the provider-proxy allowlist so capability requests and
  provider calls actually flow.
- **Upstream PR** *(target)* — capability-token-based provider
  proxy validation in `Elacity/elastos-runtime`. When this lands,
  both patches above get deleted and `UPSTREAM_VERSION` bumps to
  the release that includes the proper fix.
