# Status: draft (collides with runtime's built-in `crate::content`)

## What this is

Provider answering `elastos://content/*` with `publish / fetch /
ensure / unpublish`, wrapping kubo's HTTP API for byte storage and
mapping capsule-level policy ("network_default", "local_pin",
"transient") to pin lifecycle.

## Why it's "draft"

Per the runtime audit (upstream Elacity/elastos-runtime @ 6d4c385):
**`elastos://content/*` is already implemented inside the runtime
itself** as `crate::content` (a server-side module, not a separate
capsule subprocess). When a capsule calls `provider_call("content",
"publish", ...)`, the runtime dispatches it to its built-in module —
NOT to any installed content-provider capsule.

So installing this capsule today is a no-op on stock upstream: the
binary runs, registers, and never gets a request because the runtime
short-circuits `elastos://content/*` to its own code path before the
provider registry is consulted.

For this provider to actually serve requests we'd need ONE of:

1. **Patch the runtime to delegate to us instead of `crate::content`.**
   The cleanest split: keep the server-side module for backward
   compat, but make it a thin shim that proxies to our subprocess
   when installed. That gets the transcode + dDRM + signed
   availability work out of the runtime tree.
2. **Use a different scheme** — `elastos://hey-content/*` or
   similar. Then hey-social would have to call the new scheme name,
   which means a parallel `runtime.rs::hey_content` module.
3. **Replace the upstream `crate::content` entirely.** Requires
   removing it from the runtime; bigger PR.

Option 1 is the right long-term play. Until that lands, this capsule
is a working reference implementation that demonstrates the contract
the runtime side could delegate to.

## Wire protocol (stable, matches hey-social's `runtime::content::*` callers)

Line-delimited JSON on stdin/stdout. ProviderResponse-shaped responses.

```
init                                       → { protocol_version, provider, features }
publish   { data (b64), filename, policy } → { payload: { cid, size, filename, policy, ts },
                                                signer_did: "", signature: "" }
fetch     { cid, path? }                   → { data (b64), size, cid }
ensure    { cid, policy }                  → { cid, policy }
unpublish { cid }                          → { cid }
shutdown                                   → { message }
```

Backed by kubo HTTP at `$CONTENT_PROVIDER_KUBO_API` (default
`http://127.0.0.1:5001`). Policy mapping:
- `transient` → kubo `/add` without pin (kubo's GC eventually reaps it)
- everything else (`network_default`, `local_pin`, custom strings) → `/add` + `/pin/add`

Reserved fields `signer_did` and `signature` are empty in v0; they
hold the signed-availability-receipt content once that path is
wired.

## Smoke test (validated 2026-05-28)

```bash
echo '{"op":"init"}
{"op":"publish","data":"aGVsbG8=","filename":"hi.txt","policy":"network_default"}' \
  | CONTENT_PROVIDER_KUBO_API=http://127.0.0.1:65111 ./content-provider
```

With kubo unreachable on port 65111, returns descriptive errors:
`{"status":"error","code":"publish_failed","message":"kubo /add
request: ... Connection refused"}`. Live test against a running kubo
is out of scope for this checkout.
