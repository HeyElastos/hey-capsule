# Status: draft (not yet dispatched by stock runtime)

## What this is

Provider answering `elastos://identity/*` so app capsules can call
`whoami / sign / verify` without holding raw Ed25519 seeds in
`localStorage` or `IndexedDB`. Source-cited reasoning in the
[commit message](../../README.md) and in
`runtime_grounded_reference.md` in the memory store.

## Why it's "draft"

Per the runtime audit (upstream Elacity/elastos-runtime @ commit
6d4c385): **`identity` is not in the registered scheme list**.
`RESERVED_SUB_NAMES` (`registry.rs:163`) does not include `identity`,
so even if we install this binary the runtime's provider bus has no
mapping from `elastos://identity/*` to our subprocess.

For this provider to actually serve requests we need ONE of:

1. **Patch the runtime** to add `identity` to `RESERVED_SUB_NAMES`
   and dispatch it through the standard bridge. The
   YNH-fork's `scripts/patches/` is the right place to land this if
   we keep it Hey-local; ideally we file an upstream PR adding the
   scheme name (one-line registry change).
2. **Rename the provider's scheme to something already-reserved that
   has no built-in.** `session` is reserved + unimplemented — but
   "session" describes the runtime introspection surface, not a
   signing-key surface, so the name is misleading.
3. **Rename to a non-reserved namespace** like
   `elastos://hey-identity/*`. The runtime's registry registers
   whatever the provider's `provides` clause says; only the
   sub-dispatch shortcut for `elastos://<reserved>/*` is hard-coded.
   Top-level lookup of `hey-identity` would still work.

Option 3 is the cleanest path to making this actually run today
without touching the runtime. The wire protocol is unchanged; only
the manifest `provides` field would move.

## Wire protocol (stable)

Line-delimited JSON on stdin/stdout. ProviderResponse-shaped responses.

```
init                                  → { protocol_version, provider, features }
whoami    { namespace? }              → { did_key, public_key_hex, namespace }
sign      { namespace?, payload_b64 } → { signature_hex }
verify    { did_key, payload_b64,
            signature_hex }           → { valid }
shutdown                              → { message }
```

Master key persists at `$XDG_DATA_HOME/elastos/identity-projection-provider/master.key`
(mode 0o600); per-namespace keys are HKDF-derived as
`SHA256(master || "|" || namespace)`.

## Smoke test (validated 2026-05-28)

```bash
echo '{"op":"init"}
{"op":"whoami","namespace":"hey-social"}
{"op":"sign","namespace":"hey-social","payload_b64":"aGVsbG8="}
{"op":"verify","did_key":"<did from whoami>","payload_b64":"aGVsbG8=","signature_hex":"<sig from sign>"}' \
  | ./identity-projection-provider
```

All five responses return `{"status":"ok",...}` with a real
`did:key:z6Mk...`, a valid 64-byte signature, and verify=true on the
matching payload (verify=false on a tampered payload).
