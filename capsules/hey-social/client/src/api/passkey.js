// Hey Social passkey auth — capsule-only.
//
// WebAuthn does the real crypto in the browser; the bookkeeping
// (storing public-key credentials, minting challenges, verifying
// assertions) lives in capsule storage:
//
//   passkey-creds.json     — array of { id, publicKey, userHandle,
//                                       transports, counter, createdAt }
//   passkey-challenge.json — { challengeB64, op, ts } scoped to the most
//                            recent prompt (single-shot, cleared on use)

import {
  startRegistration,
  startAuthentication,
  browserSupportsWebAuthn,
} from "@simplewebauthn/browser";
import { storage, apiUrl, bearerReady } from "../lib/runtime";
import {
  generateAuthKey,
  hashAuthKey,
  expandKeypair,
  expandKeypairFromPRF,
  ELASTOS_IDENTITY_PRF_INPUT,
  bytesToHex,
} from "../lib/identity";
import { setSession } from "../lib/session";
import * as heyVault from "../lib/vault";

const CREDS_FILE = "passkey-creds.json";
const CHALLENGE_FILE = "passkey-challenge.json";
const PROFILE_FILE = "profile.json";

const PASSKEY_RP = {
  name: "Hey",
  id: typeof window !== "undefined" ? window.location.hostname : "localhost",
};

export const passkeySupported = () =>
  typeof window !== "undefined" && browserSupportsWebAuthn();

const randomChallenge = () => {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return bytes;
};

const b64u = {
  encode: (bytes) =>
    btoa(String.fromCharCode(...bytes))
      .replace(/\+/g, "-")
      .replace(/\//g, "_")
      .replace(/=+$/, ""),
  decode: (b64uStr) => {
    const pad = (4 - (b64uStr.length % 4)) % 4;
    const b64 = b64uStr.replace(/-/g, "+").replace(/_/g, "/") + "=".repeat(pad);
    const bin = atob(b64);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  },
};

// HKDF info label used to derive the app-specific vault key from the
// shared identity PRF output. Must match lib/vault.js for unlock-side
// symmetry. (No longer used as a PRF eval input — a second eval breaks
// dual-salt-unfriendly authenticators like the Nitrokey 3.)
const VAULT_HKDF_LABEL = "hey-social-vault-v1";

// HKDF-SHA256 expand 32 bytes from the identity PRF with a per-app label.
const deriveVaultPrf = async (identityPrf) => {
  const km = await crypto.subtle.importKey(
    "raw", identityPrf, "HKDF", false, ["deriveBits"]
  );
  const bits = await crypto.subtle.deriveBits(
    {
      name: "HKDF",
      hash: "SHA-256",
      salt: new Uint8Array(),
      info: new TextEncoder().encode(VAULT_HKDF_LABEL),
    },
    km,
    256,
  );
  return new Uint8Array(bits);
};

const readCreds = async () => (await storage.readJson(CREDS_FILE)) || [];
const writeCreds = (creds) => storage.writeJson(CREDS_FILE, creds);

const setPendingChallenge = (data) => storage.writeJson(CHALLENGE_FILE, data);
const consumeChallenge = async () => {
  const c = await storage.readJson(CHALLENGE_FILE);
  if (c) await storage.remove(CHALLENGE_FILE);
  return c;
};

const buildRegistrationOptions = async ({ name }) => {
  const challenge = randomChallenge();
  const userHandle = randomChallenge();
  await setPendingChallenge({
    challengeB64: b64u.encode(challenge),
    op: "register",
    name: name || "",
    userHandleB64: b64u.encode(userHandle),
    ts: Date.now(),
  });
  const existing = await readCreds();
  return {
    challenge: b64u.encode(challenge),
    rp: PASSKEY_RP,
    user: {
      id: b64u.encode(userHandle),
      name: name || "hey-user",
      displayName: name || "Hey user",
    },
    pubKeyCredParams: [
      { type: "public-key", alg: -8 },   // Ed25519
      { type: "public-key", alg: -7 },   // ES256
      { type: "public-key", alg: -257 }, // RS256
    ],
    timeout: 60_000,
    attestation: "none",
    authenticatorSelection: {
      residentKey: "preferred",
      userVerification: "preferred",
    },
    excludeCredentials: existing.map((c) => ({
      id: c.id,
      type: "public-key",
      transports: c.transports || [],
    })),
    extensions: {
      // Single PRF eval — every Elastos capsule asking for this same
      // input gets the same 32 bytes (cross-capsule unified identity).
      // The app-specific vault key is HKDF-derived from this output
      // (see deriveVaultPrf). Single eval keeps us compatible with
      // authenticators that reject dual-salt hmac-secret requests
      // post-UV (Nitrokey 3, some Yubikey / Windows Hello firmwares).
      prf: {
        eval: {
          first: ELASTOS_IDENTITY_PRF_INPUT,
        },
      },
    },
  };
};

// PRF output decoders. simplewebauthn surfaces clientExtensionResults
// as base64url-encoded strings; raw WebAuthn surfaces them as ArrayBuffers.
// Return Uint8Array(32) or null.
const decodePrfValue = (v) => {
  if (!v) return null;
  try {
    const bytes = typeof v === "string" ? b64u.decode(v) : new Uint8Array(v);
    return bytes.length === 32 ? bytes : null;
  } catch { return null; }
};
const prfIdentityFromResponse = (resp) =>
  decodePrfValue(resp?.clientExtensionResults?.prf?.results?.first);

export const passkeySignup = async (name) => {
  const options = await buildRegistrationOptions({ name });
  const attResp = await startRegistration({ optionsJSON: options });
  const challenge = await consumeChallenge();
  if (!challenge || challenge.op !== "register") {
    throw new Error("No pending challenge");
  }

  // Derive the signing keypair from the passkey's PRF output (first
  // eval = ELASTOS_IDENTITY_PRF_INPUT). Every Elastos capsule asking
  // this passkey for that same PRF input gets the SAME 32 bytes → same
  // keypair → same DID → unified identity across capsules. Fall back
  // to a random key only when the authenticator doesn't expose PRF
  // (older Windows Hello, hardware keys without prf extension).
  const identityPrf = prfIdentityFromResponse(attResp);
  const authKey = identityPrf
    ? bytesToHex(identityPrf)
    : generateAuthKey();
  const { didKey } = expandKeypair(authKey);
  const authKeyHash = await hashAuthKey(authKey);

  const user = {
    id: crypto.randomUUID(),
    name: (name || "").trim().slice(0, 30) || "Anonymous",
    authKeyHash,
    didKey,
    role: "general",
    avatar: "",
    bio: "",
    followers: [],
    following: [],
    pendingFollowers: [],
    pendingFollowing: [],
    createdAt: new Date().toISOString(),
  };
  await storage.writeJson(PROFILE_FILE, user);
  await setSession(authKey);

  const creds = await readCreds();
  creds.push({
    id: attResp.id,
    publicKey: attResp.response.publicKey,
    userHandle: challenge.userHandleB64,
    transports: attResp.response.transports || [],
    counter: 0,
    createdAt: new Date().toISOString(),
  });
  await writeCreds(creds);

  // Publish to the shared-identity path so any other capsule on this
  // node (hey-home shell, companion apps) sees the user as already
  // signed up — same contract as classic signUp(). Without this the
  // home shell's lock screen reads an empty Identity/profile.json
  // every boot and falls through to the signup wizard, treating the
  // user as new on every cookie clear / incognito session.
  try {
    const { writeSharedIdentity } = await import("../lib/shell");
    await writeSharedIdentity({
      name: user.name,
      didKey,
      recoveryKeyHash: authKeyHash,
      passkeys: [
        {
          id: attResp.id,
          publicKey: attResp.response.publicKey,
          userHandle: challenge.userHandleB64,
          transports: attResp.response.transports || [],
          createdAt: new Date().toISOString(),
        },
      ],
      createdAt: new Date().toISOString(),
      createdBy: "hey",
    });
  } catch (err) {
    console.warn("[hey] writeSharedIdentity failed at passkey signup", err);
  }

  // If the authenticator produced a PRF output, HKDF-derive the vault
  // key from it and initialize the vault. Failures are non-fatal:
  // signup completes; vault just isn't set up. The user can re-enroll
  // a PRF-capable passkey later to enable it.
  if (identityPrf) {
    try {
      const vaultPrf = await deriveVaultPrf(identityPrf);
      await heyVault.initVault({ prfOutput: vaultPrf, recoveryHex: authKey });
    } catch (err) {
      console.warn("[hey] vault init failed at signup", err);
    }
  } else {
    console.info(
      "[hey] passkey enrolled without PRF — vault encryption unavailable. " +
      "Hardened authenticators (Yubikey 5.7+, Touch ID on macOS 14+, " +
      "modern Windows Hello, Android 14+) support PRF."
    );
  }

  return {
    message: "User created successfully",
    user: {
      id: user.id,
      name: user.name,
      bio: "",
      avatar: "",
      role: user.role,
      didKey,
      counts: { followers: 0, following: 0 },
    },
    authKey,
    accessToken: "capsule-session",
    refreshToken: "capsule-session",
    accessTokenUpdatedAt: new Date().toISOString(),
  };
};

export const passkeyAttach = async () => {
  const me = await storage.readJson(PROFILE_FILE);
  if (!me) throw new Error("Not signed in");
  const options = await buildRegistrationOptions({ name: me.name });
  const attResp = await startRegistration({ optionsJSON: options });
  const challenge = await consumeChallenge();
  const creds = await readCreds();
  const newCred = {
    id: attResp.id,
    publicKey: attResp.response.publicKey,
    userHandle: challenge?.userHandleB64,
    transports: attResp.response.transports || [],
    counter: 0,
    createdAt: new Date().toISOString(),
  };
  creds.push(newCred);
  await writeCreds(creds);

  // Mirror the new passkey into the shared identity so the home
  // shell's lock screen can show "Sign in with passkey" for this
  // user. Without this, recovery-key users who later attach a passkey
  // only get the PIN/recovery path on the lock screen. Non-fatal on
  // failure: the attach itself succeeded.
  try {
    const { readSharedIdentity, writeSharedIdentity } = await import("../lib/shell");
    const shared = await readSharedIdentity().catch(() => null);
    if (shared?.didKey) {
      const existing = (shared.passkeys || []).filter((p) => p.id !== newCred.id);
      await writeSharedIdentity({
        ...shared,
        passkeys: [...existing, newCred],
      });
    }
  } catch (err) {
    console.warn("[hey] writeSharedIdentity failed at passkey attach", err);
  }

  return { credential: { id: attResp.id }, count: creds.length };
};

export const passkeySignin = async () => {
  const creds = await readCreds();
  if (creds.length === 0) {
    throw new Error("No passkey registered on this device");
  }
  const challenge = randomChallenge();
  await setPendingChallenge({
    challengeB64: b64u.encode(challenge),
    op: "auth",
    ts: Date.now(),
  });
  const options = {
    challenge: b64u.encode(challenge),
    rpId: PASSKEY_RP.id,
    timeout: 60_000,
    userVerification: "preferred",
    allowCredentials: creds.map((c) => ({
      id: c.id,
      type: "public-key",
      transports: c.transports || [],
    })),
    extensions: {
      // Single PRF eval — match signup. Vault PRF is HKDF-derived from
      // the identity PRF below.
      prf: {
        eval: {
          first: ELASTOS_IDENTITY_PRF_INPUT,
        },
      },
    },
  };
  const assertion = await startAuthentication({ optionsJSON: options });
  await consumeChallenge();

  // We trust the OS authenticator's UV gesture. A future hardening pass
  // should import attResp.response.publicKey as a CryptoKey at registration
  // and verify the assertion signature locally.
  const cred = creds.find((c) => c.id === assertion.id);
  if (!cred) throw new Error("Unknown credential");

  // Re-derive the signing keypair from the identity PRF and seed the
  // session. Same passkey + same PRF input → same keypair → same DID,
  // so this user is recognized identically across capsules.
  const identityPrf = prfIdentityFromResponse(assertion);
  if (identityPrf) {
    const authKey = bytesToHex(identityPrf);
    await setSession(authKey);
  }

  // If the assertion produced an identity PRF output and the user has
  // a vault, HKDF-derive the vault key from it and unwrap the master
  // key now so subsequent writeSealed / readSealed calls work.
  // Non-fatal on failure: signin completes, vault stays locked.
  if (identityPrf && (await heyVault.hasVault())) {
    try {
      const vaultPrf = await deriveVaultPrf(identityPrf);
      await heyVault.unlockVaultWithPRF(vaultPrf);
    } catch (err) {
      console.warn("[hey] vault unlock failed at signin", err);
    }
  }

  // If a Hey-local profile doesn't exist yet, synthesize one from the
  // shared identity (set up via the home welcome flow). Lets users go
  // through home welcome only and have Hey Social auto-recognize them.
  let user = await storage.readJson(PROFILE_FILE);
  if (!user) {
    const { readSharedIdentity } = await import("../lib/shell");
    const shared = await readSharedIdentity().catch(() => null);
    if (shared?.didKey) {
      user = {
        id: crypto.randomUUID(),
        name: shared.name || "Hey user",
        authKeyHash: shared.recoveryKeyHash || "",
        didKey: shared.didKey,
        role: "general",
        avatar: shared.avatar || "",
        bio: shared.bio || "",
        followers: [], following: [],
        pendingFollowers: [], pendingFollowing: [],
        createdAt: new Date().toISOString(),
      };
      await storage.writeJson(PROFILE_FILE, user);
    }
  }
  if (!user) throw new Error("No profile on this node");

  return {
    message: "Signed in via passkey",
    user: {
      id: user.id,
      name: user.name,
      bio: user.bio || "",
      avatar: user.avatar || "",
      role: user.role,
      didKey: user.didKey || "",
      counts: {
        followers: (user.followers || []).length,
        following: (user.following || []).length,
      },
    },
    accessToken: "capsule-session",
    refreshToken: "capsule-session",
    accessTokenUpdatedAt: new Date().toISOString(),
  };
};

// ── Upstream passkey contract ──────────────────────────────────────
//
// In v0.3 the runtime owns the user's principal (created via passkey
// signup in System). Hey calls upstream's authenticate flow as a
// proof-of-passkey: we get back the user's DID and (with the PRF
// extension) the same 32 bytes every other Elastos capsule's passkey
// ceremony produces for the shared identity input — so Hey's signing
// key is consistent across all the user's capsules.
//
// Flow:
//   POST /api/auth/passkey/authenticate/begin    → WebAuthn options
//   inject PRF extension                          (cross-capsule unified)
//   navigator.credentials.get(options)            → assertion
//   POST /api/auth/passkey/authenticate/complete  → upstream verifies
//
// Architecture doc: this is the stable contract the runtime tells
// third-party capsules to use ("don't read .AppData/Identity/* — use
// the passkey auth contract").

const upstreamFetch = async (path, init = {}) => {
  await bearerReady.catch(() => false);
  // Reach into runtime.js's bearer cache via the exported helpers.
  // We avoid importing the internal authHeaders to keep the surface
  // narrow; for upstream's /api/auth/passkey/* the cookie + bearer
  // path is what matters. session.current() already proved this works
  // post-bearer-exchange.
  return fetch(apiUrl(path), {
    method: init.method || "GET",
    credentials: "include",
    headers: { "Content-Type": "application/json", ...(init.headers || {}) },
    body: init.body,
  });
};

// Sign in to the runtime via the existing passkey. Returns the same
// shape passkeySignin returns ({ message, user, accessToken, ... }) so
// Landing.jsx can navigate identically after either path.
//
// Idempotent on success; safe to retry on user-cancel (an OperationError
// or NotAllowedError propagates so the caller can show a "Try again"
// affordance without leaving partial state).
export const signInViaRuntime = async (nickname = null) => {
  // 1. Ask upstream for authenticate options. Some upstream versions
  //    accept POST {}; others use GET. Try POST first (matches the
  //    /register/begin pattern); fall back to GET on 405.
  let beginResp = await upstreamFetch("/api/auth/passkey/authenticate/begin", {
    method: "POST",
    body: "{}",
  });
  if (beginResp.status === 405) {
    beginResp = await upstreamFetch("/api/auth/passkey/authenticate/begin", { method: "GET" });
  }
  if (!beginResp.ok) {
    // 400/404 most commonly means upstream has no credentials yet for
    // this principal — i.e. the user never finished passkey signup in
    // System. Surface a friendly message instead of the raw HTTP code.
    if (beginResp.status === 400 || beginResp.status === 404) {
      throw new Error(
        "No passkey set up on this device yet. Go back to System (the home dock) and create a passkey first, then come back here.",
      );
    }
    throw new Error(`passkey authenticate/begin: HTTP ${beginResp.status}`);
  }
  const beginJson = await beginResp.json();
  // Upstream may wrap options under {publicKey: ...} (WebAuthn-classic
  // shape) or hand them at the top level. SimpleWebAuthn wants the
  // top-level form.
  const options = beginJson?.publicKey || beginJson?.options || beginJson;

  // Inject the cross-capsule unified-identity PRF extension. Every
  // Elastos capsule asking for this same input gets the same 32 bytes.
  options.extensions = options.extensions || {};
  options.extensions.prf = options.extensions.prf || {
    eval: { first: ELASTOS_IDENTITY_PRF_INPUT },
  };

  // 2. Run the WebAuthn ceremony.
  const assertion = await startAuthentication({ optionsJSON: options });

  // 3. POST the assertion back to upstream to verify + finalize.
  const completeResp = await upstreamFetch("/api/auth/passkey/authenticate/complete", {
    method: "POST",
    body: JSON.stringify(assertion),
  });
  if (!completeResp.ok) {
    const txt = await completeResp.text().catch(() => "");
    throw new Error(`passkey authenticate/complete: HTTP ${completeResp.status} ${txt.slice(0, 200)}`);
  }
  // Response shape is upstream-version-dependent. We don't depend on
  // any specific DID/name field here — the PRF output is the source
  // of truth for Hey's signing identity. Upstream's response only has
  // to be "200" for us to proceed.
  let upstreamResult = null;
  try { upstreamResult = await completeResp.json(); } catch (_) {}

  // 4. Derive Hey's signing identity from the PRF output. This is the
  //    same derivation passkeySignup uses, so DID + signing key are
  //    identical to what a recovery-key signup with the same passkey
  //    would have produced — fully consistent cross-capsule identity.
  const identityPrf = prfIdentityFromResponse(assertion);
  if (!identityPrf) {
    throw new Error(
      "Passkey didn't return PRF output — your authenticator lacks the prf extension. " +
      "Use a PRF-capable passkey (Yubikey 5.7+, Touch ID macOS 14+, modern Windows Hello, Android 14+)."
    );
  }
  const authKey = bytesToHex(identityPrf);
  const { didKey } = expandKeypair(authKey);
  const authKeyHash = await hashAuthKey(authKey);
  await setSession(authKey);

  // 5. Materialize / refresh the Hey-local profile.
  const upstreamDisplayName =
    upstreamResult?.displayName ||
    upstreamResult?.name ||
    upstreamResult?.user?.name ||
    upstreamResult?.user?.displayName ||
    null;
  let user = await storage.readJson(PROFILE_FILE);
  if (!user) {
    user = {
      id: crypto.randomUUID(),
      name: (nickname || upstreamDisplayName || `${didKey.slice(0, 14)}…`).trim().slice(0, 30),
      authKeyHash,
      didKey,
      role: "general",
      avatar: "",
      bio: "",
      followers: [],
      following: [],
      pendingFollowers: [],
      pendingFollowing: [],
      createdAt: new Date().toISOString(),
    };
    await storage.writeJson(PROFILE_FILE, user);
  } else if (nickname && nickname.trim() && user.name !== nickname.trim()) {
    user.name = nickname.trim().slice(0, 30);
    await storage.writeJson(PROFILE_FILE, user);
  }

  // 6. Publish to the shared-identity contract so the home shell and
  //    other capsules see the user as already signed up. Dual-write to
  //    both canonical and legacy paths via writeSharedIdentity.
  try {
    const { writeSharedIdentity } = await import("../lib/shell");
    await writeSharedIdentity({
      name: user.name,
      didKey,
      recoveryKeyHash: authKeyHash,
      passkeys: [],
      createdAt: user.createdAt,
      createdBy: "hey-runtime-signin",
    });
  } catch (err) {
    console.warn("[hey] writeSharedIdentity failed at runtime signin", err);
  }

  // 7. If a vault exists, unlock it from the same PRF output.
  if (await heyVault.hasVault()) {
    try {
      const vaultPrf = await deriveVaultPrf(identityPrf);
      await heyVault.unlockVaultWithPRF(vaultPrf);
    } catch (err) {
      console.warn("[hey] vault unlock failed at runtime signin", err);
    }
  } else {
    // No vault yet (first-ever sign-in) — initialize one bound to this
    // passkey PRF + the user's recovery-equivalent (authKey hex).
    try {
      const vaultPrf = await deriveVaultPrf(identityPrf);
      await heyVault.initVault({ prfOutput: vaultPrf, recoveryHex: authKey });
    } catch (err) {
      console.warn("[hey] vault init failed at runtime signin", err);
    }
  }

  return {
    message: "Signed in via runtime passkey",
    user: {
      id: user.id,
      name: user.name,
      bio: user.bio || "",
      avatar: user.avatar || "",
      role: user.role,
      didKey,
      counts: {
        followers: (user.followers || []).length,
        following: (user.following || []).length,
      },
    },
    accessToken: "capsule-session",
    refreshToken: "capsule-session",
    accessTokenUpdatedAt: new Date().toISOString(),
  };
};

export const listPasskeys = async () => {
  const creds = await readCreds();
  return {
    credentials: creds.map((c) => ({
      id: c.id,
      createdAt: c.createdAt,
      transports: c.transports || [],
    })),
  };
};

export const removePasskey = async (credId) => {
  const creds = await readCreds();
  const filtered = creds.filter((c) => c.id !== credId);
  await writeCreds(filtered);
  return { removed: creds.length - filtered.length };
};
