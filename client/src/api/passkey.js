import axios from "axios";
import {
  startRegistration,
  startAuthentication,
  browserSupportsWebAuthn,
} from "@simplewebauthn/browser";
import { isCapsuleMode } from "../lib/mode";
import { storage as runtimeStorage } from "../lib/runtime";
import {
  generateAuthKey,
  hashAuthKey,
  expandKeypair,
} from "../lib/identity";
import { setSession } from "../lib/session";

const API = axios.create({ baseURL: "/api" });
const authHeaders = (token) => ({ Authorization: `Bearer ${token}` });

export const passkeySupported = () =>
  typeof window !== "undefined" && browserSupportsWebAuthn();

// ─── Capsule-mode passkey storage ──────────────────────────────────
//
// WebAuthn does the real crypto in the browser; the server's role was
// bookkeeping — store public-key credentials, mint challenges, verify
// assertions. In capsule mode that bookkeeping moves to localhost storage:
//
//   passkey-creds.json     — array of { id, publicKey, userHandle, transports,
//                                       counter, createdAt }
//   passkey-challenge.json — { challengeB64, op, ts } scoped to the most
//                            recent prompt (single-shot, cleared on use)

const CREDS_FILE = "passkey-creds.json";
const CHALLENGE_FILE = "passkey-challenge.json";
const PROFILE_FILE = "profile.json";

const PASSKEY_RP = {
  name: "Hey",
  id: typeof window !== "undefined" ? window.location.hostname : "localhost",
};

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
};

const readCreds = async () =>
  (await runtimeStorage.readJson(CREDS_FILE)) || [];
const writeCreds = (creds) => runtimeStorage.writeJson(CREDS_FILE, creds);
const setPendingChallenge = (data) =>
  runtimeStorage.writeJson(CHALLENGE_FILE, data);
const consumeChallenge = async () => {
  const c = await runtimeStorage.readJson(CHALLENGE_FILE);
  if (c) await runtimeStorage.remove(CHALLENGE_FILE);
  return c;
};

const capsuleBuildRegistrationOptions = async ({ name }) => {
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
  };
};

const capsulePasskeySignup = async (name) => {
  const options = await capsuleBuildRegistrationOptions({ name });
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
  await runtimeStorage.writeJson(PROFILE_FILE, user);
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

const capsulePasskeyAttach = async () => {
  const me = await runtimeStorage.readJson(PROFILE_FILE);
  if (!me) throw new Error("Not signed in");
  const options = await capsuleBuildRegistrationOptions({ name: me.name });
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

const capsulePasskeySignin = async () => {
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
  };
  const assertion = await startAuthentication({ optionsJSON: options });
  await consumeChallenge();

  // We trust the OS authenticator's UV gesture. A future hardening pass
  // would import attResp.response.publicKey as a CryptoKey at registration
  // and verify the assertion signature locally.
  const cred = creds.find((c) => c.id === assertion.id);
  if (!cred) throw new Error("Unknown credential");

  const user = await runtimeStorage.readJson(PROFILE_FILE);
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

const capsuleListPasskeys = async () => {
  const creds = await readCreds();
  return {
    credentials: creds.map((c) => ({
      id: c.id,
      createdAt: c.createdAt,
      transports: c.transports || [],
    })),
  };
};

const capsuleRemovePasskey = async (credId) => {
  const creds = await readCreds();
  const filtered = creds.filter((c) => c.id !== credId);
  await writeCreds(filtered);
  return { removed: creds.length - filtered.length };
};

// ────────────────────────────────────────────────────────────────────

export const passkeySignup = async (name) => {
  if (isCapsuleMode()) return capsulePasskeySignup(name);
  const start = await API.post("/passkey/register/options", { name });
  const { challengeId, options } = start.data;
  const attResp = await startRegistration({ optionsJSON: options });
  const verify = await API.post("/passkey/register/verify", {
    challengeId,
    response: attResp,
  });
  return verify.data;
};

export const passkeyAttach = async (token) => {
  if (isCapsuleMode()) return capsulePasskeyAttach();
  const start = await API.post(
    "/passkey/register/options",
    {},
    { headers: authHeaders(token) }
  );
  const { challengeId, options } = start.data;
  const attResp = await startRegistration({ optionsJSON: options });
  const verify = await API.post(
    "/passkey/register/verify",
    { challengeId, response: attResp },
    { headers: authHeaders(token) }
  );
  return verify.data;
};

export const listPasskeys = async (token) => {
  if (isCapsuleMode()) return capsuleListPasskeys();
  const response = await API.get("/passkey/credentials", { headers: authHeaders(token) });
  return response.data;
};

export const removePasskey = async (credId, token) => {
  if (isCapsuleMode()) return capsuleRemovePasskey(credId);
  const response = await API.delete(`/passkey/credentials/${encodeURIComponent(credId)}`, {
    headers: authHeaders(token),
  });
  return response.data;
};

export const passkeySignin = async () => {
  if (isCapsuleMode()) return capsulePasskeySignin();
  const start = await API.post("/passkey/auth/options");
  const { challengeId, options } = start.data;
  const authResp = await startAuthentication({ optionsJSON: options });
  const verify = await API.post("/passkey/auth/verify", {
    challengeId,
    response: authResp,
  });
  return verify.data;
};
