import { describe, it, expect } from "vitest";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const {
  keypairFromAuthKey,
  publicKeyToRawBytes,
  publicKeyToDidKey,
  didKeyToPublicKey,
  sign,
  verify,
} = require("./identity.js");

const VALID_KEY = "a".repeat(64);
const VALID_KEY_2 = "b".repeat(64);

describe("keypairFromAuthKey", () => {
  it("returns Node KeyObjects for a valid 32-byte hex key", () => {
    const { privateKey, publicKey } = keypairFromAuthKey(VALID_KEY);
    expect(privateKey.asymmetricKeyType).toBe("ed25519");
    expect(publicKey.asymmetricKeyType).toBe("ed25519");
  });

  it("is deterministic — same authKey always yields the same pubkey", () => {
    const a = keypairFromAuthKey(VALID_KEY);
    const b = keypairFromAuthKey(VALID_KEY);
    expect(publicKeyToRawBytes(a.publicKey).toString("hex"))
      .toBe(publicKeyToRawBytes(b.publicKey).toString("hex"));
  });

  it("different authKeys produce different pubkeys", () => {
    const a = keypairFromAuthKey(VALID_KEY);
    const b = keypairFromAuthKey(VALID_KEY_2);
    expect(publicKeyToRawBytes(a.publicKey).toString("hex"))
      .not.toBe(publicKeyToRawBytes(b.publicKey).toString("hex"));
  });

  it("rejects malformed authKeys", () => {
    expect(() => keypairFromAuthKey("")).toThrow();
    expect(() => keypairFromAuthKey("nothex")).toThrow();
    expect(() => keypairFromAuthKey("a".repeat(63))).toThrow(); // too short
    expect(() => keypairFromAuthKey("g".repeat(64))).toThrow(); // not hex
    expect(() => keypairFromAuthKey(null)).toThrow();
  });
});

describe("did:key round-trip", () => {
  it("encodes a public key to did:key:z... format", () => {
    const { publicKey } = keypairFromAuthKey(VALID_KEY);
    const didKey = publicKeyToDidKey(publicKey);
    expect(didKey.startsWith("did:key:z")).toBe(true);
    // Ed25519 did:key strings are ~56 chars total: "did:key:z" (9) +
    // base58 of [0xed,0x01,...32 pubkey bytes] (46-47 chars).
    expect(didKey.length).toBeGreaterThanOrEqual(50);
    expect(didKey.length).toBeLessThanOrEqual(60);
  });

  it("decodes back to the same public key", () => {
    const { publicKey } = keypairFromAuthKey(VALID_KEY);
    const didKey = publicKeyToDidKey(publicKey);
    const decoded = didKeyToPublicKey(didKey);
    expect(publicKeyToRawBytes(decoded).toString("hex"))
      .toBe(publicKeyToRawBytes(publicKey).toString("hex"));
  });

  it("rejects strings that aren't did:key:z...", () => {
    expect(() => didKeyToPublicKey("did:web:example.com")).toThrow();
    expect(() => didKeyToPublicKey("not-a-did")).toThrow();
    expect(() => didKeyToPublicKey("")).toThrow();
  });

  it("rejects did:key with a non-Ed25519 multicodec", () => {
    // 0x01 0x00 instead of 0xed 0x01 — wrong prefix
    const fake = "did:key:z" + "1xQF";
    expect(() => didKeyToPublicKey(fake)).toThrow();
  });
});

describe("sign + verify", () => {
  it("verifies a signature it just produced", () => {
    const { privateKey, publicKey } = keypairFromAuthKey(VALID_KEY);
    const msg = "hello, federation";
    const sig = sign(msg, privateKey);
    expect(verify(msg, sig, publicKey)).toBe(true);
  });

  it("works with Buffer inputs", () => {
    const { privateKey, publicKey } = keypairFromAuthKey(VALID_KEY);
    const msg = Buffer.from([1, 2, 3, 4]);
    const sig = sign(msg, privateKey);
    expect(verify(msg, sig, publicKey)).toBe(true);
  });

  it("rejects a signature after the message is tampered with", () => {
    const { privateKey, publicKey } = keypairFromAuthKey(VALID_KEY);
    const sig = sign("original", privateKey);
    expect(verify("tampered", sig, publicKey)).toBe(false);
  });

  it("rejects a signature from a different key", () => {
    const a = keypairFromAuthKey(VALID_KEY);
    const b = keypairFromAuthKey(VALID_KEY_2);
    const sig = sign("msg", a.privateKey);
    expect(verify("msg", sig, b.publicKey)).toBe(false);
  });

  it("rejects an obviously malformed signature without throwing", () => {
    const { publicKey } = keypairFromAuthKey(VALID_KEY);
    expect(verify("msg", "00".repeat(63), publicKey)).toBe(false); // wrong length
    expect(verify("msg", "deadbeef", publicKey)).toBe(false);
  });
});

describe("verify across the wire (sign here, did:key decode + verify)", () => {
  it("simulates the federation verify path", () => {
    // Alice signs locally with her recovery key.
    const alice = keypairFromAuthKey(VALID_KEY);
    const aliceDidKey = publicKeyToDidKey(alice.publicKey);
    const event = JSON.stringify({ type: "post", body: "first post" });
    const sig = sign(event, alice.privateKey);

    // Bob receives just (event, sig, sender_id=did:key). He doesn't know
    // Alice's KeyObject, only her did:key string.
    const bobView = didKeyToPublicKey(aliceDidKey);
    expect(verify(event, sig, bobView)).toBe(true);
  });
});

describe("interop sanity: known Ed25519 test vector", () => {
  // RFC 8032 Section 7.1, Test 1. The canonical Ed25519 vector — if our
  // pubkey derivation or signing diverges from RFC 8032 even slightly,
  // we wouldn't be interop-compatible with anyone else verifying us.
  it("matches RFC 8032 Section 7.1 Test 1 (empty message)", () => {
    const seed = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
    const { privateKey, publicKey } = keypairFromAuthKey(seed);

    expect(publicKeyToRawBytes(publicKey).toString("hex")).toBe(
      "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"
    );

    const sig = sign(Buffer.alloc(0), privateKey);
    expect(sig).toBe(
      "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
    );

    expect(verify(Buffer.alloc(0), sig, publicKey)).toBe(true);
  });
});
