// IPLD helpers for Hey Social posts.
//
// Posts have two parts:
//
//   1. Immutable content (caption, media list, author, ts) — encoded
//      as a dag-cbor IPLD blob and pinned to IPFS. The blob's CID is
//      the canonical post identity. Anyone with the CID can verify
//      the bytes and re-fetch from any IPFS gateway. Same media
//      reposted by two people = one IPFS pin.
//
//   2. Mutable overlays (reactions, comments, reposts) — kept in the
//      local Hey storage cache, NOT in the IPLD blob. CRDT-style
//      merge across nodes is Phase 2 work (probably iroh-docs).
//
// Carrier event for a new post is a thin envelope:
//
//   { type: "post.create.v2", payload: { post_cid: "bafy…" } }
//
// Receivers fetch the CID from IPFS via ipfs.getBytes, decode here,
// then materialize a local post record with empty overlay arrays.
//
// Schema is versioned (v: 1 inside the blob) so future shape changes
// can be additive without breaking existing readers.

import * as dagCbor from "@ipld/dag-cbor";

export const IPLD_POST_SCHEMA_VERSION = 1;

// Build the immutable post body. Strips out mutable fields so the CID
// is stable across reactions/comments/edits to overlay state.
//
// Input shape (Hey's internal post record): full post object as built
// by createPost. We pluck only the immutable bits.
//
// Output shape (dag-cbor-friendly): plain JS object with string CIDs
// for media. We intentionally keep CIDs as strings here rather than
// proper CID objects — keeps decoders simpler and the bytes layout
// is the same in dag-cbor either way (string is just a tagged type).
const buildImmutableBody = (post) => {
  if (!post || typeof post !== "object") {
    throw new Error("ipld: post must be an object");
  }
  if (!post.userDid || typeof post.userDid !== "string") {
    throw new Error("ipld: post.userDid required");
  }
  if (typeof post.ts !== "number") {
    throw new Error("ipld: post.ts (number) required");
  }
  const media = Array.isArray(post.images) ? post.images.map((m) => ({
    cid: String(m.cid || ""),
    type: m.type === "video" ? "video" : "photo",
    mime: String(m.mime || ""),
    name: String(m.name || ""),
  })) : [];
  return {
    v: IPLD_POST_SCHEMA_VERSION,
    author_did: post.userDid,
    author_name: String(post.userName || ""),
    caption: String(post.caption || ""),
    media,
    ts: post.ts,
    created_at: post.createdAt || new Date(post.ts).toISOString(),
  };
};

// Encode a post into dag-cbor bytes. Returns Uint8Array ready for
// ipfs.addBytes(). Throws on invalid input.
export const encodePostMetadata = (post) => {
  const body = buildImmutableBody(post);
  return dagCbor.encode(body);
};

// Decode dag-cbor bytes back into the immutable post body. Returns the
// shape produced by buildImmutableBody. Throws on bad encoding /
// unsupported schema version.
export const decodePostMetadata = (bytes) => {
  const decoded = dagCbor.decode(bytes);
  if (!decoded || typeof decoded !== "object") {
    throw new Error("ipld: decoded value is not an object");
  }
  if (decoded.v !== IPLD_POST_SCHEMA_VERSION) {
    throw new Error(`ipld: unsupported post schema v=${decoded.v}`);
  }
  return decoded;
};

// Convenience for receivers: given a decoded IPLD body + a fresh CID,
// produce a Hey-internal post record with empty overlay arrays.
// Caller is responsible for generating the local id (UUID), and for
// filling avatar from the local peer cache once the bundle is known.
export const materializeFromIpld = (body, { post_cid, id }) => ({
  id: id || (typeof crypto !== "undefined" && crypto.randomUUID ? crypto.randomUUID() : `post-${Date.now()}`),
  post_cid,
  userId: body.author_did,
  userDid: body.author_did,
  userName: body.author_name || `${body.author_did.slice(0, 14)}…`,
  userAvatar: "",
  caption: body.caption,
  images: body.media.map((m) => ({
    cid: m.cid,
    url: `elastos://${m.cid}`,
    type: m.type,
    mime: m.mime,
    name: m.name,
  })),
  createdAt: body.created_at,
  ts: body.ts,
  reactions: {},
  reposts: [],
  comments: [],
});
