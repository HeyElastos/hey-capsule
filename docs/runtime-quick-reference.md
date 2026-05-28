# Runtime Contract — Quick Reference

> Index of the most-load-bearing truths from
> [runtime-contract.md](runtime-contract.md). Audit dated **2026-05-28**
> against upstream commit
> [`6d4c385`](https://github.com/Elacity/elastos-runtime/commit/6d4c385).
> If you're an AI agent or new contributor, start with this page;
> drill into the full doc only when you need source citations.

## Key truths that override prior assumptions

- **Launch token schema is `elastos.home.launch-token/v2`** (not v1). The signature domain string IS `elastos.home.launch.v1` (intentional — domain stable for sig compat). Source: `elastos/crates/elastos-server/src/api/gateway_home_token.rs:142`.

- **`/api/apps/<id>/runtime-token` IS NOT an upstream route.** Searching upstream returns zero matches. If Hey was calling it, that path was YNH-patched or capsule-fabricated. Our `redeem_launch_token` fallback won't hit anything on stock upstream.

- **`/api/apps/<id>/session/start` exists ONLY for chat-room in upstream.** It's not a generic per-app route. Source: `gateway_room.rs:78-108`. For hey-social to use this pattern we'd need either a runtime patch adding a generic handler OR a YNH-side route.

- **The gateway provider proxy has a SEPARATE allowlist from the redemption endpoint.** This is the gate that makes a 401 → 403 with Hey today. Upstream v0.3's `gateway_provider_proxy.rs` hardcodes `allowed_apps = {documents, library, system, wallet, browser}` for `/api/provider/{peer,content,did,ipfs,...}/<op>`. YNH-fork patch 0001 opens `/runtime-token` (so we can mint a bearer) but does NOT add hey-social to the provider proxy allowlist. Net: Hey can authenticate; every Carrier / content / DID call still 403s. Three options: extend the fork patch, file upstream PR, or accept no real capability access. Cannot be bypassed in-capsule.

- **Cookies are `home-session`, `room-session`, `browser-session`** — all HttpOnly, Path=/, SameSite=Lax, +Secure under TLS. Source: `gateway.rs:118-120`, `gateway_home_token.rs:66-79`.

- **All `/api/localhost/Users/*` direct HTTP access returns 403 unconditionally.** `reject_principal_root_storage_path` runs BEFORE capability check. The capsule must NOT hit `/api/localhost/Users/self/...` paths directly; principal-scoped writes go through `/api/provider/<scheme>/<op>` so the runtime can inject `principal_id`. Source: `handlers/storage.rs:504-511`.

- **No auto-grant capability policy shipping today.** All `/api/capability/request` go to `pending`. `permissions.*` in capsule.json is intent for manifest validation + shell UI, NOT an authorization. Dev workaround: `ELASTOS_SHADOW_MODE=on` enables shadow auto-grant.

- **Reserved doesn't mean shipped.** `RESERVED_SUB_NAMES` (registry.rs:163) lists `peer, did, ai, llama, ipfs, content, tunnel, storage, namespace, message, chain, net, exit, browser-engine, wallet, drm, rights, key, decrypt, availability` — but upstream's tree has NO peer-provider, NO session-provider, NO principal-provider, NO capabilities-provider, NO identity-provider. We're greenfield for any of these.

- **`elastos://content/*` is in the runtime as `crate::content`**, not as a separate capsule. Our [content-provider](capsules/content-provider/) is a duplicate code path; only one can own the scheme at a time.

- **Chat-room does NOT use the DM marker convention.** It uses runtime room-service objects (not gossip). The DM marker `\x01DM:<recipient_pubkey>\x01<content>` is from the native `chat` (microvm) capsule.

- **Provider subprocesses are 30s-timeout, single-mutex serialized.** stdin/stdout JSON, no env vars, no fd, no signals. Source: `bridge.rs:20-26`.

- **CapsuleManifest is `#[serde(deny_unknown_fields)]`** — any typo in capsule.json fails parsing.

- **`role=provider` REQUIRES `authority: { reason, capabilities[], audit_events[] }`** per manifest.rs:271-292. Our [identity-projection-provider/capsule.json](capsules/identity-projection-provider/capsule.json) and [content-provider/capsule.json](capsules/content-provider/capsule.json) are MISSING this — manifest validation would reject them on install. Fix needed.

- **Provider binaries: capsule.json says `type: microvm` + `entrypoint: rootfs.ext4` but the supervisor uses `*_PROVIDER_BIN` env vars** (set in `_common.sh:413-440`) to find the actual binary. The rootfs.ext4 file may not exist in dev; that's fine for native-binary providers.

- **Principal format: `person:local:<hex16(sha256(proof_binding_id))>`** (auth.rs:1156-1158). Or `device:<did>` for device principals. **NEVER a `did:key:` — Hey's `inherit_session` DID-only filter correctly excludes it.**

- **Principal-root storage path: `localhost://Users/<hex12(sha256(principal_id))>/...`** (auth.rs:1175). NOT `Users/self/...` on the wire — the `self` alias is rewritten by the runtime at the principal-aware bridge.

## Provider scheme map (built-in vs greenfield)

Built-in (shipped, working): `localhost`, `did`, `ipfs` (system-only), `ai`, `llama`, `tunnel`, `net`, `exit`, `decrypt`, `key`, `drm`, `rights`, `wallet`, `chain`, `availability`, `webspace`, `browser-engine`, `site`.

Server-side built-ins (no capsule): `content` (crate::content), `namespace` (handlers::namespace).

Reserved but not shipped: `peer`, `message`, `storage`, `session`, `principal`, `capabilities`, `identity`. Anything you build here is brand new.

## File index for drill-down

Upstream paths (raw.githubusercontent.com prefix `https://raw.githubusercontent.com/Elacity/elastos-runtime/6d4c385/`):
- Provider proxy + auth: `elastos/crates/elastos-server/src/api/handlers/provider.rs`, `gateway_provider_proxy.rs`, `middleware.rs`
- Launch token: `gateway_home_token.rs`, `gateway_room.rs`
- Storage handler: `handlers/storage.rs`
- Capability: `handlers/capability.rs`, `capability/{token,manager}.rs`
- Manifest: `elastos-common/src/manifest.rs`
- Provider registry: `runtime/src/provider/{registry,bridge}.rs`
- Auth/principal: `elastos-server/src/auth.rs`, `elastos-auth/src/lib.rs`
- Canonical reference impls: `elastos/capsules/localhost-provider/src/main.rs` (typed enum), `capsules/did-provider/src/main.rs` (free-form), `capsules/chat-room/browser/index.html` (session/start client)

Local: `/var/home/linux/ai/elastos-runtime-ynh/` (UPSTREAM_VERSION v0.3.0); scripts/_common.sh for build + env wiring; components.additions.json for binary registration.

## How to apply

When wiring a new capsule or provider, before assuming any HTTP/wire shape:
1. Check this doc's "Key truths" list first
2. Read the relevant section in [runtime-contract.md](runtime-contract.md)
3. If still uncertain, fetch the cited source file from upstream
4. Cross-check against a canonical reference impl (`localhost-provider` for storage-shaped, `did-provider` for free-form RPC)
5. Check existing in-pack examples — every provider in [`capsules/*-provider/`](../capsules/) has a `STATUS.md` describing wire-shape + what works on stock upstream
