const crypto = require("crypto");
const jwt = require("jsonwebtoken");
const {
  generateRegistrationOptions,
  verifyRegistrationResponse,
  generateAuthenticationOptions,
  verifyAuthenticationResponse,
} = require("@simplewebauthn/server");
const { readDb, writeDb } = require("../utils/db");

const { SECRET, REFRESH_SECRET } = require("../utils/secrets");
const RP_ID = process.env.RP_ID || "localhost";
const RP_NAME = process.env.RP_NAME || "Hey";
const ORIGIN = process.env.WEBAUTHN_ORIGIN || "http://localhost:3000";

// Ephemeral challenge store. challengeId -> { challenge, name?, userId?, expiresAt }
const challenges = new Map();
const CHALLENGE_TTL_MS = 5 * 60 * 1000;

const sweepChallenges = () => {
  const now = Date.now();
  for (const [id, val] of challenges) {
    if (val.expiresAt < now) challenges.delete(id);
  }
};
setInterval(sweepChallenges, 60_000).unref();

const signTokens = (user) => {
  const payload = { id: user.id, name: user.name };
  return {
    accessToken: jwt.sign(payload, SECRET, { expiresIn: "6h" }),
    refreshToken: jwt.sign(payload, REFRESH_SECRET, { expiresIn: "7d" }),
  };
};

const ensureSocial = (user) => {
  if (!Array.isArray(user.followers)) user.followers = [];
  if (!Array.isArray(user.following)) user.following = [];
  if (!Array.isArray(user.pendingFollowers)) user.pendingFollowers = [];
  if (!Array.isArray(user.pendingFollowing)) user.pendingFollowing = [];
  if (!Array.isArray(user.credentials)) user.credentials = [];
};

const publicUser = (user) => {
  ensureSocial(user);
  return {
    id: user.id,
    name: user.name,
    bio: user.bio || "",
    avatar: user.avatar || "",
    role: user.role,
    counts: { followers: user.followers.length, following: user.following.length },
  };
};

// userId is stored in WebAuthn as a Uint8Array (a.k.a. userHandle). We use the
// existing user uuid (hex) directly as bytes.
const idToBytes = (id) => Buffer.from(id.replace(/-/g, ""), "hex");
const bytesToId = (buf) => {
  const hex = Buffer.from(buf).toString("hex");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20, 32)}`;
};

// ------- Registration (signup OR attach to existing) -------

const registerOptions = async (req, res) => {
  try {
    const db = await readDb();
    let user;
    let isNew = false;
    let userName;
    let userId;

    if (req.user) {
      user = db.users.find((u) => u.id === req.user.id);
      if (!user) return res.status(404).json({ message: "User not found" });
      ensureSocial(user);
      userId = user.id;
      userName = user.name;
    } else {
      const name = (req.body?.name || "").trim().slice(0, 30);
      if (!name) return res.status(400).json({ message: "Name is required" });
      if (db.users.some((u) => (u.name || "").toLowerCase() === name.toLowerCase())) {
        return res.status(409).json({ message: "Name already in use" });
      }
      userId = crypto.randomUUID();
      userName = name;
      isNew = true;
    }

    const excludeCredentials = user
      ? (user.credentials || []).map((c) => ({
          id: c.id,
          transports: c.transports || [],
        }))
      : [];

    const options = await generateRegistrationOptions({
      rpName: RP_NAME,
      rpID: RP_ID,
      userID: idToBytes(userId),
      userName,
      userDisplayName: userName,
      attestationType: "none",
      excludeCredentials,
      authenticatorSelection: {
        residentKey: "preferred",
        userVerification: "preferred",
      },
    });

    const challengeId = crypto.randomUUID();
    challenges.set(challengeId, {
      challenge: options.challenge,
      expiresAt: Date.now() + CHALLENGE_TTL_MS,
      userId,
      userName,
      isNew,
    });

    return res.status(200).json({ challengeId, options });
  } catch (error) {
    return res.status(500).json({ message: "Could not start registration" });
  }
};

const registerVerify = async (req, res) => {
  try {
    const { challengeId, response } = req.body || {};
    if (!challengeId || !response) {
      return res.status(400).json({ message: "Missing challenge or response" });
    }
    const entry = challenges.get(challengeId);
    if (!entry || entry.expiresAt < Date.now()) {
      challenges.delete(challengeId);
      return res.status(400).json({ message: "Challenge expired" });
    }

    const verification = await verifyRegistrationResponse({
      response,
      expectedChallenge: entry.challenge,
      expectedOrigin: ORIGIN,
      expectedRPID: RP_ID,
      requireUserVerification: false, // can't require on register — depends on device
    });

    if (!verification.verified || !verification.registrationInfo) {
      return res.status(400).json({ message: "Verification failed" });
    }

    const { credential } = verification.registrationInfo;
    const newCred = {
      id: credential.id,
      publicKey: Buffer.from(credential.publicKey).toString("base64url"),
      counter: credential.counter,
      transports: response.response?.transports || [],
      createdAt: new Date().toISOString(),
    };

    const db = await readDb();

    if (entry.isNew) {
      if (db.users.some((u) => (u.name || "").toLowerCase() === entry.userName.toLowerCase())) {
        return res.status(409).json({ message: "Name already in use" });
      }
      const newUser = {
        id: entry.userId,
        name: entry.userName,
        bio: "",
        avatar: "",
        followers: [],
        following: [],
        pendingFollowers: [],
        pendingFollowing: [],
        credentials: [newCred],
        createdAt: new Date().toISOString(),
      };
      db.users.push(newUser);
      await writeDb(db);
      challenges.delete(challengeId);
      const tokens = signTokens(newUser);
      return res.status(200).json({ user: publicUser(newUser), ...tokens });
    }

    // Attach to existing user
    const user = db.users.find((u) => u.id === entry.userId);
    if (!user) return res.status(404).json({ message: "User not found" });
    ensureSocial(user);
    if ((user.credentials || []).some((c) => c.id === newCred.id)) {
      challenges.delete(challengeId);
      return res.status(409).json({ message: "Passkey already registered" });
    }
    user.credentials.push(newCred);
    await writeDb(db);
    challenges.delete(challengeId);
    return res.status(200).json({ user: publicUser(user) });
  } catch (error) {
    return res.status(500).json({ message: "Could not verify registration" });
  }
};

// ------- Authentication (signin) -------

const authOptions = async (req, res) => {
  try {
    const options = await generateAuthenticationOptions({
      rpID: RP_ID,
      userVerification: "preferred",
      // empty allowCredentials → discoverable credentials only
    });

    const challengeId = crypto.randomUUID();
    challenges.set(challengeId, {
      challenge: options.challenge,
      expiresAt: Date.now() + CHALLENGE_TTL_MS,
    });

    return res.status(200).json({ challengeId, options });
  } catch (error) {
    return res.status(500).json({ message: "Could not start authentication" });
  }
};

const authVerify = async (req, res) => {
  try {
    const { challengeId, response } = req.body || {};
    if (!challengeId || !response) {
      return res.status(400).json({ message: "Missing challenge or response" });
    }
    const entry = challenges.get(challengeId);
    if (!entry || entry.expiresAt < Date.now()) {
      challenges.delete(challengeId);
      return res.status(400).json({ message: "Challenge expired" });
    }

    const db = await readDb();

    // Look up credential. With discoverable credentials, the response includes
    // userHandle (the user.id we stored at registration).
    const userHandleB64 = response.response?.userHandle;
    let user;
    let storedCred;
    if (userHandleB64) {
      const userId = bytesToId(Buffer.from(userHandleB64, "base64url"));
      user = db.users.find((u) => u.id === userId);
      if (user) storedCred = (user.credentials || []).find((c) => c.id === response.id);
    } else {
      for (const u of db.users) {
        const c = (u.credentials || []).find((x) => x.id === response.id);
        if (c) { user = u; storedCred = c; break; }
      }
    }

    if (!user || !storedCred) {
      return res.status(404).json({ message: "Passkey not recognized" });
    }

    const verification = await verifyAuthenticationResponse({
      response,
      expectedChallenge: entry.challenge,
      expectedOrigin: ORIGIN,
      expectedRPID: RP_ID,
      credential: {
        id: storedCred.id,
        publicKey: Buffer.from(storedCred.publicKey, "base64url"),
        counter: storedCred.counter,
        transports: storedCred.transports || [],
      },
      requireUserVerification: false, // many synced passkeys flag UV per-device
    });

    if (!verification.verified) {
      return res.status(401).json({ message: "Verification failed" });
    }

    storedCred.counter = verification.authenticationInfo.newCounter;
    await writeDb(db);
    challenges.delete(challengeId);
    const tokens = signTokens(user);
    return res.status(200).json({ user: publicUser(user), ...tokens });
  } catch (error) {
    return res.status(500).json({ message: "Could not verify authentication" });
  }
};

module.exports = { registerOptions, registerVerify, authOptions, authVerify };
