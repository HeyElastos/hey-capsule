// Identity primitives — Rust port of capsules/hey-social/client/src/lib/identity.js.
//
// Same algorithm: 32-byte auth key (the PRF output, hex-encoded) reinterpreted
// as an Ed25519 seed; did:key encoding per W3C CCG (base58btc + multicodec
// ed25519-pub prefix 0xed 0x01). Produces the exact same did:key strings as
// the JS version for the same input bytes — required for cross-capsule
// identity continuity.

use ed25519_compact::{KeyPair, PublicKey, Seed, Signature};
use sha2::{Digest, Sha256};

const BASE58_ALPHABET: &[u8] =
    b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

const ED25519_PUB_MULTICODEC: [u8; 2] = [0xed, 0x01];

// The cross-capsule unified-identity input. Every Elastos capsule asking the
// passkey for this same PRF input gets the same 32 bytes, hence the same DID.
// MUST match capsules/hey-social/client/src/lib/identity.js exactly.
pub const ELASTOS_IDENTITY_PRF_INPUT: &[u8] = b"elastos-identity-v1";

pub fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

pub fn hex_to_bytes(hex: &str) -> Result<Vec<u8>, String> {
    if hex.len() % 2 != 0 {
        return Err("hex length must be even".into());
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    for i in (0..hex.len()).step_by(2) {
        let byte = u8::from_str_radix(&hex[i..i + 2], 16)
            .map_err(|e| format!("invalid hex: {e}"))?;
        out.push(byte);
    }
    Ok(out)
}

fn base58_encode(buf: &[u8]) -> String {
    if buf.is_empty() {
        return String::new();
    }
    // Big-integer base conversion. Same logic as the JS reference; we use
    // a Vec<u8> as our bignum (base 256, big-endian) and repeatedly divide
    // by 58.
    let mut digits: Vec<u8> = buf.to_vec();
    let mut out = Vec::<u8>::new();
    let mut start = 0usize;
    while start < digits.len() {
        // Divide the big-endian number `digits[start..]` by 58, recording remainder.
        let mut remainder = 0u32;
        let mut new_start = start;
        let mut seen_nonzero = false;
        for i in start..digits.len() {
            let cur = remainder * 256 + digits[i] as u32;
            let q = cur / 58;
            remainder = cur % 58;
            digits[i] = q as u8;
            if !seen_nonzero {
                if q == 0 {
                    new_start = i + 1;
                } else {
                    seen_nonzero = true;
                }
            }
        }
        out.push(BASE58_ALPHABET[remainder as usize]);
        start = new_start;
    }
    // Leading-zero compensation: JS code prepends one '1' per leading zero byte.
    for b in buf {
        if *b != 0 {
            break;
        }
        out.push(b'1');
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

pub fn public_key_to_did_key(public_key: &[u8; 32]) -> String {
    let mut prefixed = [0u8; 34];
    prefixed[..2].copy_from_slice(&ED25519_PUB_MULTICODEC);
    prefixed[2..].copy_from_slice(public_key);
    format!("did:key:z{}", base58_encode(&prefixed))
}

pub struct Expanded {
    pub seed: [u8; 32],
    pub public_key: [u8; 32],
    pub did_key: String,
}

// Inverse: parse "did:key:z..." back to the 32-byte Ed25519 public key.
pub fn did_key_to_public_key(did_key: &str) -> Result<[u8; 32], String> {
    let s = did_key.strip_prefix("did:key:z").ok_or("not a did:key:z...")?;
    let bytes = base58_decode(s)?;
    if bytes.len() != 34 || bytes[0] != 0xed || bytes[1] != 0x01 {
        return Err("not an Ed25519 did:key".into());
    }
    let mut pk = [0u8; 32];
    pk.copy_from_slice(&bytes[2..]);
    Ok(pk)
}

fn base58_decode(s: &str) -> Result<Vec<u8>, String> {
    if s.is_empty() {
        return Ok(Vec::new());
    }
    // Build a base-58 → digit table.
    let mut table = [255u8; 128];
    for (i, b) in BASE58_ALPHABET.iter().enumerate() {
        table[*b as usize] = i as u8;
    }
    // Bignum accumulator (base 256, big-endian).
    let mut acc: Vec<u8> = Vec::new();
    for c in s.bytes() {
        if c >= 128 {
            return Err(format!("invalid base58 char: {}", c as char));
        }
        let digit = table[c as usize];
        if digit == 255 {
            return Err(format!("invalid base58 char: {}", c as char));
        }
        // acc = acc * 58 + digit
        let mut carry = digit as u32;
        for byte in acc.iter_mut().rev() {
            let v = *byte as u32 * 58 + carry;
            *byte = (v & 0xff) as u8;
            carry = v >> 8;
        }
        while carry != 0 {
            acc.insert(0, (carry & 0xff) as u8);
            carry >>= 8;
        }
    }
    // Leading '1's in input → leading zero bytes in output.
    let leading_ones = s.bytes().take_while(|c| *c == b'1').count();
    let mut out = vec![0u8; leading_ones];
    out.extend(acc);
    Ok(out)
}

// SHA-256 of the auth key hex — what the server stores as authKeyHash.
pub fn hash_auth_key_hex(auth_key_hex: &str) -> String {
    let mut h = Sha256::new();
    h.update(auth_key_hex.as_bytes());
    bytes_to_hex(&h.finalize())
}

// Sign arbitrary bytes with the Ed25519 seed. Returns hex sig (64 bytes).
// Deterministic mode (noise=None) matches the JS reference's noble.sign
// behavior — same input always produces the same signature, so verifiers
// don't see signature churn across re-publishes.
pub fn sign(message: &[u8], seed: &[u8; 32]) -> String {
    let kp = KeyPair::from_seed(Seed::new(*seed));
    let sig: Signature = kp.sk.sign(message, None);
    bytes_to_hex(sig.as_ref())
}

pub fn verify(message: &[u8], signature_hex: &str, public_key: &[u8; 32]) -> bool {
    let sig_bytes = match hex_to_bytes(signature_hex) {
        Ok(b) if b.len() == 64 => b,
        _ => return false,
    };
    let sig = match Signature::from_slice(&sig_bytes) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let pk = PublicKey::new(*public_key);
    pk.verify(message, &sig).is_ok()
}

// Same contract as JS expandKeypair: input is a 64-char hex string (32 bytes),
// output is { seed, publicKey, didKey }. Deterministic — same hex always
// produces the same did:key.
pub fn expand_keypair(auth_key_hex: &str) -> Result<Expanded, String> {
    let seed_vec = hex_to_bytes(auth_key_hex)?;
    if seed_vec.len() != 32 {
        return Err(format!(
            "auth_key must be 32 bytes (64 hex chars), got {}",
            seed_vec.len()
        ));
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&seed_vec);
    let kp = KeyPair::from_seed(Seed::new(seed));
    let pk_bytes: [u8; 32] = *kp.pk;
    Ok(Expanded {
        seed,
        public_key: pk_bytes,
        did_key: public_key_to_did_key(&pk_bytes),
    })
}
