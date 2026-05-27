// Profile bundles — how peers learn each other's encryption pubkeys.
//
// Each user publishes a signed "profile.bundle" event to their own
// Carrier topic on sign-in. Anyone wanting to E2E-encrypt to them
// subscribes to that topic, fetches the latest bundle, verifies the
// signature, and caches it locally.
//
// Topic: hey-v0/profile/<did>
// Event payload shape:
//   { name?, x25519Pub: <hex>, kemPub: <hex> }
//
// Mirrors hey-messenger-capsule's profile.js with topic-namespace
// `hey-v0/profile/` to fit Hey's existing topic convention.

import { peer } from "./runtime";
import { createSignedEvent, verifySignedEvent } from "./events";
import { getKeypair, getDidKey } from "./session";

const BUNDLE_CACHE_LS = "hey-profile-bundles";

const profileTopic = (did) => `hey-v0/profile/${did}`;

const hexToBytes = (hex) => {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
};

const bytesToHex = (bytes) => {
  let hex = "";
  for (let i = 0; i < bytes.length; i++) hex += bytes[i].toString(16).padStart(2, "0");
  return hex;
};

const loadBundleCache = () => {
  try { return JSON.parse(localStorage.getItem(BUNDLE_CACHE_LS) || "{}"); }
  catch { return {}; }
};
const saveBundleCache = (m) => {
  try { localStorage.setItem(BUNDLE_CACHE_LS, JSON.stringify(m)); } catch {}
};

// Publish own profile bundle to Carrier. Call once after sign-in.
export const publishOwnBundle = async ({ name } = {}) => {
  const kp = getKeypair();
  if (!kp?.x25519?.publicKey || !kp?.kem?.publicKey) {
    // Not signed in, or session predates the PQ upgrade. Skip silently —
    // older sessions migrate when the user signs in again.
    return null;
  }
  const payload = {
    name: name || null,
    x25519Pub: bytesToHex(kp.x25519.publicKey),
    kemPub: bytesToHex(kp.kem.publicKey),
  };
  const event = await createSignedEvent({ type: "profile.bundle", payload }, kp);
  await peer.joinTopic(profileTopic(kp.didKey));
  await peer.publish({
    topic: profileTopic(kp.didKey),
    message: JSON.stringify(event),
    sender_id: event.sender_did,
    ts: event.ts,
    signature: event.signature,
  });
  return event;
};

// Resolve a peer's pubkey bundle by DID. Returns { x25519Pub, kemPub }
// or null. Cached locally so we don't re-fetch on every send.
export const resolveBundle = async (did) => {
  const cache = loadBundleCache();
  if (cache[did]) {
    return {
      x25519Pub: hexToBytes(cache[did].x25519Pub),
      kemPub: hexToBytes(cache[did].kemPub),
      cached: true,
    };
  }

  try {
    await peer.joinTopic(profileTopic(did));
  } catch {
    return null;
  }
  let resp;
  try {
    resp = await peer.recv({
      topic: profileTopic(did),
      limit: 5,
      consumer_id: `hey:profile:${getDidKey() || "anon"}`,
    });
  } catch {
    return null;
  }
  const items = resp?.data?.messages || resp?.messages || [];
  for (const item of items) {
    let event;
    try { event = JSON.parse(item.message ?? item); } catch { continue; }
    if (event?.type !== "profile.bundle") continue;
    if (event?.sender_did !== did) continue;
    const v = verifySignedEvent(event);
    if (!v.valid) continue;
    const { x25519Pub, kemPub } = event.payload || {};
    if (typeof x25519Pub !== "string" || typeof kemPub !== "string") continue;
    cache[did] = { x25519Pub, kemPub, at: Date.now() };
    saveBundleCache(cache);
    return {
      x25519Pub: hexToBytes(x25519Pub),
      kemPub: hexToBytes(kemPub),
      cached: false,
    };
  }
  return null;
};

export const forgetBundle = (did) => {
  const cache = loadBundleCache();
  delete cache[did];
  saveBundleCache(cache);
};
