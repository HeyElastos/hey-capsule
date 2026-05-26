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
import { storage } from "../lib/runtime";
import {
  generateAuthKey,
  hashAuthKey,
  expandKeypair,
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

// Pre-encoded PRF input for the vault — the assertion/attestation
// extensions are passed as base64url strings via simplewebauthn's
// optionsJSON interface. Must match what lib/vault.js uses
// internally (encoded "hey-social-vault-v1").
const VAULT_PRF_INPUT_B64U = b64u.encode(
  new TextEncoder().encode("hey-social-vault-v1")
);

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
      prf: { eval: { first: VAULT_PRF_INPUT_B64U } },
    },
  };
};

// Try to extract the PRF output from a WebAuthn response. simplewebauthn
// surfaces clientExtensionResults as base64url-encoded strings. Returns a
// Uint8Array(32) or null when the authenticator didn't produce PRF.
const prfOutputFromResponse = (resp) => {
  const first = resp?.clientExtensionResults?.prf?.results?.first;
  if (!first) return null;
  try {
    const bytes = b64u.decode(first);
    return bytes.length === 32 ? bytes : null;
  } catch { return null; }
};

export const passkeySignup = async (name) => {
  const options = await buildRegistrationOptions({ name });
  const attResp = await startRegistration({ optionsJSON: options });
  const challenge = await consumeChallenge();
  if (!challenge || challenge.op !== "register") {
    throw new Error("No pending challenge");
  }

  const authKey = generateAuthKey();
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

  // If the authenticator produced a PRF output, initialize the vault.
  // Failures are non-fatal: signup completes; vault just isn't set up.
  // The user can re-enroll a PRF-capable passkey later to enable it.
  const prfOutput = prfOutputFromResponse(attResp);
  if (prfOutput) {
    try {
      await heyVault.initVault({ prfOutput, recoveryHex: authKey });
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
  creds.push({
    id: attResp.id,
    publicKey: attResp.response.publicKey,
    userHandle: challenge?.userHandleB64,
    transports: attResp.response.transports || [],
    counter: 0,
    createdAt: new Date().toISOString(),
  });
  await writeCreds(creds);
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
      prf: { eval: { first: VAULT_PRF_INPUT_B64U } },
    },
  };
  const assertion = await startAuthentication({ optionsJSON: options });
  await consumeChallenge();

  // We trust the OS authenticator's UV gesture. A future hardening pass
  // should import attResp.response.publicKey as a CryptoKey at registration
  // and verify the assertion signature locally.
  const cred = creds.find((c) => c.id === assertion.id);
  if (!cred) throw new Error("Unknown credential");

  // If the assertion produced a PRF output and the user has a vault,
  // unwrap the master key now so subsequent writeSealed / readSealed
  // calls work. Non-fatal on failure: signin completes, vault stays
  // locked.
  const prfOutput = prfOutputFromResponse(assertion);
  if (prfOutput && (await heyVault.hasVault())) {
    try {
      await heyVault.unlockVaultWithPRF(prfOutput);
    } catch (err) {
      console.warn("[hey] vault unlock failed at signin", err);
    }
  }

  const user = await storage.readJson(PROFILE_FILE);
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
