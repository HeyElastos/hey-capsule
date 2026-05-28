// Hybrid post-quantum E2E encryption for DMs.
//
// Rust port of capsules/hey-messenger/client/src/lib/pqcrypto.js. Same
// construction, byte-identical envelope shape, so a hey-messenger client
// and a hey-social-rust client can read each other's messages.
//
//   shared_secret = HKDF-SHA256(X25519_dh || ML-KEM-768_secret, info=HKDF_INFO)
//   ciphertext    = ChaCha20-Poly1305(plaintext, key=shared_secret, nonce)
//
// Why hybrid:
//   * ML-KEM-768 is the NIST FIPS 203 post-quantum KEM standard. The
//     RustCrypto ml-kem crate is the pure-Rust implementation.
//   * X25519 is the classical fallback. An attacker would have to break
//     BOTH primitives to recover plaintext. Same hybrid pattern Signal
//     PQXDH and the NIST PQ migration guidelines recommend.
//
// Single-shot per-message encryption — no key ratchet (Phase 2 / matches
// the React reference's status). Per-message FS via an ephemeral X25519
// keypair the sender generates and includes in the envelope.
//
// Wire format (every byte field base64-encoded in the JSON envelope):
//   { v: "hpq-1", eph: <32B>, kem: <1088B>, n: <12B>, ct: <varB> }

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key as ChachaKey, Nonce};
use hkdf::Hkdf;
use ml_kem::kem::{Decapsulate, Encapsulate};
use ml_kem::{Ciphertext, EncodedSizeUser, KemCore, MlKem768};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use x25519_dalek::{PublicKey as X25519Pub, StaticSecret as X25519Priv};

const HKDF_INFO: &[u8] = b"hey-messenger/hpq-1";
pub const ENVELOPE_VERSION: &str = "hpq-1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HpqEnvelope {
    pub v: String,
    pub eph: String, // base64 — 32B X25519 pub (ephemeral)
    pub kem: String, // base64 — ML-KEM-768 ciphertext (1088B)
    pub n: String,   // base64 — 12B nonce
    pub ct: String,  // base64 — ChaCha20-Poly1305 ciphertext + tag
}

/// Per-user persistent keypairs. The X25519 private is the user's
/// Ed25519 seed (we derive X25519 from the same 32 bytes — different
/// curve math, both stay strong). ML-KEM is generated fresh once and
/// persisted alongside the session.
#[derive(Debug, Clone)]
pub struct UserKeys {
    pub x25519_priv: [u8; 32],
    pub x25519_pub: [u8; 32],
    pub ml_kem_secret_bytes: Vec<u8>, // ~2400B
    pub ml_kem_public_bytes: Vec<u8>, // 1184B
}

/// Public projection — what we publish to peers via the profile bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicKeys {
    pub x25519_pub_b64: String,
    pub ml_kem_pub_b64: String,
}

impl UserKeys {
    pub fn public(&self) -> PublicKeys {
        PublicKeys {
            x25519_pub_b64: B64.encode(self.x25519_pub),
            ml_kem_pub_b64: B64.encode(&self.ml_kem_public_bytes),
        }
    }
}

/// Derive an X25519 keypair from an Ed25519 seed. The X25519 pubkey is
/// independent of the Ed25519 pubkey (different curve math). Both can
/// be derived from the same 32-byte seed without weakening either.
pub fn x25519_from_seed(seed: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    let priv_key = X25519Priv::from(*seed);
    let pub_key = X25519Pub::from(&priv_key);
    (*priv_key.as_bytes(), *pub_key.as_bytes())
}

/// Generate a fresh ML-KEM-768 keypair. Each user generates one at
/// first signin and persists it — the pubkey gets published via the
/// profile bundle.
pub fn generate_ml_kem_keypair() -> (Vec<u8>, Vec<u8>) {
    let mut rng = OsRng;
    let (dk, ek) = MlKem768::generate(&mut rng);
    (dk.as_bytes().to_vec(), ek.as_bytes().to_vec())
}

/// Build / load the full user-key bundle from an Ed25519 seed (hex auth_key).
pub fn keys_from_seed_and_kem(seed: &[u8; 32], ml_kem_secret: &[u8], ml_kem_public: &[u8]) -> UserKeys {
    let (priv_bytes, pub_bytes) = x25519_from_seed(seed);
    UserKeys {
        x25519_priv: priv_bytes,
        x25519_pub: pub_bytes,
        ml_kem_secret_bytes: ml_kem_secret.to_vec(),
        ml_kem_public_bytes: ml_kem_public.to_vec(),
    }
}

fn derive_key(x25519_secret: &[u8], kem_secret: &[u8]) -> [u8; 32] {
    let mut ikm = Vec::with_capacity(x25519_secret.len() + kem_secret.len());
    ikm.extend_from_slice(x25519_secret);
    ikm.extend_from_slice(kem_secret);
    let hk = Hkdf::<Sha256>::new(None, &ikm);
    let mut out = [0u8; 32];
    hk.expand(HKDF_INFO, &mut out).expect("hkdf expand");
    out
}

/// Encrypt to a recipient identified by their X25519 + ML-KEM-768 public
/// keys. Recipient must have previously published both pubkeys.
pub fn encrypt_to_hybrid(
    plaintext: &str,
    recipient_x25519_pub: &[u8; 32],
    recipient_kem_pub_bytes: &[u8],
) -> Result<HpqEnvelope, String> {
    // Ephemeral X25519 keypair — fresh per message for partial forward secrecy.
    let mut eph_seed = [0u8; 32];
    OsRng.fill_bytes(&mut eph_seed);
    let eph_priv = X25519Priv::from(eph_seed);
    eph_seed.fill(0);
    let eph_pub = X25519Pub::from(&eph_priv);
    let recipient_pub = X25519Pub::from(*recipient_x25519_pub);
    let x25519_secret = eph_priv.diffie_hellman(&recipient_pub);

    // ML-KEM-768 encapsulation against the recipient's KEM pubkey.
    let ek = <<MlKem768 as KemCore>::EncapsulationKey as EncodedSizeUser>::from_bytes(
        recipient_kem_pub_bytes
            .try_into()
            .map_err(|_| "ml-kem encapsulation key wrong size".to_string())?,
    );
    let (kem_ct, kem_secret) = ek
        .encapsulate(&mut OsRng)
        .map_err(|e| format!("ml-kem encapsulate: {e:?}"))?;

    let key = derive_key(x25519_secret.as_bytes(), &kem_secret);

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let cipher = ChaCha20Poly1305::new(ChachaKey::from_slice(&key));
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| format!("chacha encrypt: {e:?}"))?;

    let kem_bytes: &[u8] = kem_ct.as_slice();
    Ok(HpqEnvelope {
        v: ENVELOPE_VERSION.into(),
        eph: B64.encode(eph_pub.as_bytes()),
        kem: B64.encode(kem_bytes),
        n: B64.encode(nonce_bytes),
        ct: B64.encode(ct),
    })
}

/// Decrypt an envelope using our X25519 private + ML-KEM secret.
pub fn decrypt_hybrid(env: &HpqEnvelope, keys: &UserKeys) -> Result<String, String> {
    if env.v != ENVELOPE_VERSION {
        return Err(format!("unsupported envelope version: {}", env.v));
    }
    let eph_pub_bytes: [u8; 32] = B64
        .decode(&env.eph)
        .map_err(|e| format!("eph b64: {e}"))?
        .try_into()
        .map_err(|_| "eph wrong size".to_string())?;
    let kem_ct = B64.decode(&env.kem).map_err(|e| format!("kem b64: {e}"))?;
    let nonce_bytes: [u8; 12] = B64
        .decode(&env.n)
        .map_err(|e| format!("nonce b64: {e}"))?
        .try_into()
        .map_err(|_| "nonce wrong size".to_string())?;
    let ct = B64.decode(&env.ct).map_err(|e| format!("ct b64: {e}"))?;

    let our_priv = X25519Priv::from(keys.x25519_priv);
    let eph_pub = X25519Pub::from(eph_pub_bytes);
    let x25519_secret = our_priv.diffie_hellman(&eph_pub);

    let dk = <<MlKem768 as KemCore>::DecapsulationKey as EncodedSizeUser>::from_bytes(
        keys.ml_kem_secret_bytes
            .as_slice()
            .try_into()
            .map_err(|_| "ml-kem decapsulation key wrong size".to_string())?,
    );
    let kem_ct_arr: Ciphertext<MlKem768> = Ciphertext::<MlKem768>::clone_from_slice(&kem_ct);
    let kem_secret = dk
        .decapsulate(&kem_ct_arr)
        .map_err(|e| format!("ml-kem decapsulate: {e:?}"))?;

    let key = derive_key(x25519_secret.as_bytes(), &kem_secret);

    let cipher = ChaCha20Poly1305::new(ChachaKey::from_slice(&key));
    let nonce = Nonce::from_slice(&nonce_bytes);
    let pt = cipher
        .decrypt(nonce, ct.as_ref())
        .map_err(|e| format!("chacha decrypt (likely auth tag mismatch): {e:?}"))?;
    String::from_utf8(pt).map_err(|e| format!("plaintext not utf-8: {e}"))
}

/// Round-trip self-test. Run from a wasm debug console to sanity-check
/// the crypto stack:  `crypto::self_test()` should return `Ok(true)`.
pub fn self_test() -> Result<bool, String> {
    let mut seed = [0u8; 32];
    OsRng.fill_bytes(&mut seed);
    let (priv_b, pub_b) = x25519_from_seed(&seed);
    let (kem_secret, kem_public) = generate_ml_kem_keypair();
    let keys = UserKeys {
        x25519_priv: priv_b,
        x25519_pub: pub_b,
        ml_kem_secret_bytes: kem_secret,
        ml_kem_public_bytes: kem_public,
    };
    let env = encrypt_to_hybrid("hello, post-quantum world 🔒", &keys.x25519_pub, &keys.ml_kem_public_bytes)?;
    let out = decrypt_hybrid(&env, &keys)?;
    if out != "hello, post-quantum world 🔒" {
        return Err(format!("self_test mismatch: {out}"));
    }
    Ok(true)
}
