//! identity-projection-provider — the OS service that owns a Hey user's
//! signing identity. Capsules ask "who am I", "sign this", "what are my
//! pubkeys", "do this ECDH / KEM-decapsulate" instead of holding an
//! Ed25519 seed (and X25519 + ML-KEM material) in localStorage.
//!
//! IDENTITY MODEL (per-principal, runtime-custodial — the wallet model):
//! the key is derived from the authenticated runtime PRINCIPAL the gateway
//! injects (`principal_id`, e.g. `person:local:…`) plus a namespace. So the
//! SAME user gets the SAME did:key across every Hey capsule, no passkey tap,
//! and two different users on one box get different keys. (Pre-patch-0006,
//! when the gateway doesn't inject principal_id yet, we fall back to a fixed
//! principal so the provider still runs — degraded to per-install, not
//! per-user.) The seed never leaves this process.
//!
//! Wire protocol mirrors did-/blobs-/wallet-provider: line-delimited JSON on
//! stdin/stdout, registered by server_infra.rs via register_sub_provider
//! (patches 0004/0005 — exactly how `wallet` is registered upstream).
//!
//! Operations (all key-bearing ops take an injected `principal_id`):
//!   init                                              -> { protocol_version, provider, features }
//!   whoami    { principal_id?, namespace? }           -> { did_key, public_key_hex }
//!   pubkeys   { principal_id?, namespace? }           -> { x25519_pub_b64, ml_kem_pub_b64, did_key }
//!   sign      { principal_id?, namespace?, payload_b64 } -> { signature_hex }
//!   x25519_dh { principal_id?, namespace?, eph_pub_b64 } -> { shared_b64 }
//!   ml_kem_decapsulate { principal_id?, namespace?, ct_b64 } -> { shared_b64 }
//!   verify    { did_key, payload_b64, signature_hex } -> { valid }
//!   shutdown                                          -> ok
//!
//! Crypto MUST match hey-chat/src/crypto.rs exactly (ml-kem 0.2, x25519-dalek
//! 2, Ed25519): x25519 is derived from the Ed25519 seed (same 32 bytes), the
//! ML-KEM keypair is generated deterministically from a per-principal seed so
//! it is stable across restarts (peers cache the public key).
//!
//! Storage:
//!   $XDG_DATA_HOME/elastos/identity-projection-provider/master.key
//!     — 32-byte random root; per-(principal,namespace) seeds are HKDF/SHA-256
//!       derived from it. Never exported. mode 0o600.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use ml_kem::kem::Decapsulate;
use ml_kem::{Ciphertext, EncodedSizeUser, KemCore, MlKem768};
use parking_lot::Mutex;
use rand_chacha::ChaCha20Rng;
use rand_core::{OsRng, RngCore, SeedableRng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use x25519_dalek::{PublicKey as XPub, StaticSecret as XSecret};

const DEFAULT_NAMESPACE: &str = "default";
/// Used when the gateway hasn't injected a principal yet (pre-patch-0006).
/// Keeps the provider working — degraded to a single per-install identity.
const FALLBACK_PRINCIPAL: &str = "local:default";
const ED25519_PUB_MULTICODEC: [u8; 2] = [0xed, 0x01];

// ── Wire ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    Init {},
    Whoami {
        #[serde(default)]
        principal_id: Option<String>,
        #[serde(default)]
        namespace: Option<String>,
    },
    Pubkeys {
        #[serde(default)]
        principal_id: Option<String>,
        #[serde(default)]
        namespace: Option<String>,
    },
    Sign {
        #[serde(default)]
        principal_id: Option<String>,
        #[serde(default)]
        namespace: Option<String>,
        payload_b64: String,
    },
    X25519Dh {
        #[serde(default)]
        principal_id: Option<String>,
        #[serde(default)]
        namespace: Option<String>,
        eph_pub_b64: String,
    },
    MlKemDecapsulate {
        #[serde(default)]
        principal_id: Option<String>,
        #[serde(default)]
        namespace: Option<String>,
        ct_b64: String,
    },
    Verify {
        did_key: String,
        payload_b64: String,
        signature_hex: String,
    },
    Shutdown {},
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Response {
    Ok {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data: Option<serde_json::Value>,
    },
    Error {
        code: String,
        message: String,
    },
}

impl Response {
    fn ok(data: serde_json::Value) -> Self {
        Self::Ok { data: Some(data) }
    }
    fn err_code(code: impl Into<String>, msg: impl Into<String>) -> Self {
        Self::Error {
            code: code.into(),
            message: msg.into(),
        }
    }
}

// ── Per-principal key bundle ─────────────────────────────────────────

/// All key material for one (principal, namespace). The Ed25519 seed roots
/// both signing and the X25519 key (same 32 bytes, as hey-chat does); the
/// ML-KEM keypair is derived deterministically from a separate per-principal
/// seed. Held only in this process.
struct KeyBundle {
    ed: SigningKey,
    x_secret: XSecret,
    ml_dk_bytes: Vec<u8>, // ML-KEM-768 decapsulation (secret) key
    ml_ek_bytes: Vec<u8>, // ML-KEM-768 encapsulation (public) key
}

impl KeyBundle {
    fn did_key(&self) -> String {
        public_key_to_did_key(self.ed.verifying_key().as_bytes())
    }
    fn x25519_pub(&self) -> [u8; 32] {
        XPub::from(&self.x_secret).to_bytes()
    }
    /// ECDH against an ephemeral X25519 pubkey — the recipient half of
    /// hey-chat's sealed-sender decrypt. Returns the 32-byte shared secret.
    fn x25519_dh(&self, eph_pub: [u8; 32]) -> String {
        let shared = self.x_secret.diffie_hellman(&XPub::from(eph_pub));
        B64.encode(shared.as_bytes())
    }
    /// ML-KEM-768 decapsulation — the recipient half of the KEM. `ct_bytes`
    /// is the 1088-byte ciphertext from the envelope; returns the shared key.
    fn ml_kem_decapsulate(&self, ct_bytes: &[u8]) -> Result<String, String> {
        let dk = <<MlKem768 as KemCore>::DecapsulationKey as EncodedSizeUser>::from_bytes(
            self.ml_dk_bytes
                .as_slice()
                .try_into()
                .map_err(|_| "decapsulation key wrong size".to_string())?,
        );
        let ct = Ciphertext::<MlKem768>::try_from(ct_bytes)
            .map_err(|_| format!("ML-KEM-768 ciphertext wrong size ({} bytes)", ct_bytes.len()))?;
        let shared = dk
            .decapsulate(&ct)
            .map_err(|e| format!("decapsulate: {e:?}"))?;
        let bytes: &[u8] = &shared;
        Ok(B64.encode(bytes))
    }
}

struct Node {
    master_secret: [u8; 32],
    cache: Arc<Mutex<HashMap<String, Arc<KeyBundle>>>>,
}

impl Node {
    async fn spawn(data_dir: PathBuf) -> Result<Self> {
        tokio::fs::create_dir_all(&data_dir).await?;
        let master_secret = load_or_create_master(&data_dir).await?;
        Ok(Self {
            master_secret,
            cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    fn bundle(&self, principal: &str, namespace: &str) -> Arc<KeyBundle> {
        let cache_key = format!("{principal}\u{1f}{namespace}");
        if let Some(b) = self.cache.lock().get(&cache_key) {
            return b.clone();
        }
        let bundle = Arc::new(derive_bundle(&self.master_secret, principal, namespace));
        self.cache.lock().insert(cache_key, bundle.clone());
        bundle
    }
}

/// Domain-separated SHA-256 over (master || tag || principal || namespace).
fn derive_seed(master: &[u8; 32], tag: &[u8], principal: &str, namespace: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(master);
    h.update([0x1f]);
    h.update(tag);
    h.update([0x1f]);
    h.update(principal.as_bytes());
    h.update([0x1f]);
    h.update(namespace.as_bytes());
    h.finalize().into()
}

fn derive_bundle(master: &[u8; 32], principal: &str, namespace: &str) -> KeyBundle {
    let ed_seed = derive_seed(master, b"ed25519", principal, namespace);
    let ed = SigningKey::from_bytes(&ed_seed);
    // X25519 from the SAME seed as Ed25519 — mirrors hey-chat::crypto::x25519_from_seed.
    let x_secret = XSecret::from(ed_seed);

    // ML-KEM-768 generated from a deterministic RNG seeded per-principal, so
    // the public key is stable across restarts (peers cache it).
    let ml_seed = derive_seed(master, b"ml-kem-768", principal, namespace);
    let mut rng = ChaCha20Rng::from_seed(ml_seed);
    let (dk, ek) = MlKem768::generate(&mut rng);

    KeyBundle {
        ed,
        x_secret,
        ml_dk_bytes: dk.as_bytes().to_vec(),
        ml_ek_bytes: ek.as_bytes().to_vec(),
    }
}

// ── did:key (W3C CCG, base58btc + ed25519-pub multicodec) ────────────

fn public_key_to_did_key(public_key: &[u8; 32]) -> String {
    let mut prefixed = [0u8; 34];
    prefixed[..2].copy_from_slice(&ED25519_PUB_MULTICODEC);
    prefixed[2..].copy_from_slice(public_key);
    format!("did:key:z{}", base58_encode(&prefixed))
}

const BASE58_ALPHABET: &[u8] =
    b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

fn base58_encode(buf: &[u8]) -> String {
    if buf.is_empty() {
        return String::new();
    }
    let mut digits: Vec<u8> = buf.to_vec();
    let mut out = Vec::<u8>::new();
    let mut start = 0usize;
    while start < digits.len() {
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
    for b in buf {
        if *b != 0 {
            break;
        }
        out.push(b'1');
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

fn did_key_to_public_key(did_key: &str) -> Option<[u8; 32]> {
    let s = did_key.strip_prefix("did:key:z")?;
    let bytes = base58_decode(s)?;
    if bytes.len() != 34 || bytes[0] != 0xed || bytes[1] != 0x01 {
        return None;
    }
    let mut pk = [0u8; 32];
    pk.copy_from_slice(&bytes[2..]);
    Some(pk)
}

fn base58_decode(s: &str) -> Option<Vec<u8>> {
    if s.is_empty() {
        return Some(Vec::new());
    }
    let mut table = [255u8; 128];
    for (i, b) in BASE58_ALPHABET.iter().enumerate() {
        table[*b as usize] = i as u8;
    }
    let mut acc: Vec<u8> = Vec::new();
    for c in s.bytes() {
        if c >= 128 {
            return None;
        }
        let digit = table[c as usize];
        if digit == 255 {
            return None;
        }
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
    let leading_ones = s.bytes().take_while(|c| *c == b'1').count();
    let mut out = vec![0u8; leading_ones];
    out.extend(acc);
    Some(out)
}

// ── Master-secret persistence ────────────────────────────────────────

async fn load_or_create_master(data_dir: &PathBuf) -> Result<[u8; 32]> {
    let path = data_dir.join("master.key");
    if let Ok(bytes) = tokio::fs::read(&path).await {
        let decoded = B64.decode(&bytes).context("decode master")?;
        let arr: [u8; 32] = decoded
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("master key wrong size"))?;
        return Ok(arr);
    }
    let mut buf = [0u8; 32];
    OsRng.fill_bytes(&mut buf);
    let encoded = B64.encode(buf);
    tokio::fs::write(&path, encoded.as_bytes())
        .await
        .context("write master")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = tokio::fs::metadata(&path).await {
            let mut perms = meta.permissions();
            perms.set_mode(0o600);
            let _ = tokio::fs::set_permissions(&path, perms).await;
        }
    }
    Ok(buf)
}

fn data_dir() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".local/share")
        });
    base.join("elastos/identity-projection-provider")
}

// ── Dispatch ─────────────────────────────────────────────────────────

fn principal_of(p: &Option<String>) -> String {
    match p {
        Some(s) if !s.is_empty() => s.clone(),
        _ => FALLBACK_PRINCIPAL.to_string(),
    }
}

fn ns_of(n: &Option<String>) -> String {
    match n {
        Some(s) if !s.is_empty() => s.clone(),
        _ => DEFAULT_NAMESPACE.to_string(),
    }
}

async fn handle(node: &tokio::sync::Mutex<Option<Node>>, req: Request) -> Response {
    match req {
        Request::Init {} => {
            let mut guard = node.lock().await;
            if guard.is_none() {
                match Node::spawn(data_dir()).await {
                    Ok(n) => *guard = Some(n),
                    Err(e) => return Response::err_code("init_failed", format!("{e:#}")),
                }
            }
            Response::ok(serde_json::json!({
                "protocol_version": "0.2",
                "provider": "identity",
                "features": ["whoami", "pubkeys", "sign", "x25519_dh", "ml_kem_decapsulate", "verify"],
            }))
        }
        Request::Whoami {
            principal_id,
            namespace,
        } => with_node(node, |n| {
            let b = n.bundle(&principal_of(&principal_id), &ns_of(&namespace));
            Response::ok(serde_json::json!({
                "did_key": b.did_key(),
                "public_key_hex": hex::encode(b.ed.verifying_key().as_bytes()),
            }))
        })
        .await,
        Request::Pubkeys {
            principal_id,
            namespace,
        } => with_node(node, |n| {
            let b = n.bundle(&principal_of(&principal_id), &ns_of(&namespace));
            Response::ok(serde_json::json!({
                "x25519_pub_b64": B64.encode(b.x25519_pub()),
                "ml_kem_pub_b64": B64.encode(&b.ml_ek_bytes),
                "did_key": b.did_key(),
            }))
        })
        .await,
        Request::Sign {
            principal_id,
            namespace,
            payload_b64,
        } => {
            let payload = match B64.decode(&payload_b64) {
                Ok(p) => p,
                Err(e) => return Response::err_code("bad_payload", format!("base64: {e}")),
            };
            with_node(node, |n| {
                let b = n.bundle(&principal_of(&principal_id), &ns_of(&namespace));
                let sig: Signature = b.ed.sign(&payload);
                Response::ok(serde_json::json!({ "signature_hex": hex::encode(sig.to_bytes()) }))
            })
            .await
        }
        Request::X25519Dh {
            principal_id,
            namespace,
            eph_pub_b64,
        } => {
            let eph: [u8; 32] = match B64.decode(&eph_pub_b64).ok().and_then(|v| v.try_into().ok()) {
                Some(e) => e,
                None => return Response::err_code("bad_eph", "eph_pub_b64 not 32 bytes base64"),
            };
            with_node(node, |n| {
                let b = n.bundle(&principal_of(&principal_id), &ns_of(&namespace));
                Response::ok(serde_json::json!({ "shared_b64": b.x25519_dh(eph) }))
            })
            .await
        }
        Request::MlKemDecapsulate {
            principal_id,
            namespace,
            ct_b64,
        } => {
            let ct_bytes = match B64.decode(&ct_b64) {
                Ok(c) => c,
                Err(e) => return Response::err_code("bad_ct", format!("base64: {e}")),
            };
            with_node(node, |n| {
                let b = n.bundle(&principal_of(&principal_id), &ns_of(&namespace));
                match b.ml_kem_decapsulate(&ct_bytes) {
                    Ok(shared_b64) => {
                        Response::ok(serde_json::json!({ "shared_b64": shared_b64 }))
                    }
                    Err(e) => Response::err_code("decapsulate_failed", e),
                }
            })
            .await
        }
        Request::Verify {
            did_key,
            payload_b64,
            signature_hex,
        } => {
            let payload = match B64.decode(&payload_b64) {
                Ok(p) => p,
                Err(e) => return Response::err_code("bad_payload", format!("base64: {e}")),
            };
            let sig_bytes = match hex::decode(&signature_hex) {
                Ok(b) => b,
                Err(e) => return Response::err_code("bad_sig", format!("hex: {e}")),
            };
            let sig = match Signature::from_slice(&sig_bytes) {
                Ok(s) => s,
                Err(e) => return Response::err_code("bad_sig", format!("sig parse: {e}")),
            };
            let pk = match did_key_to_public_key(&did_key) {
                Some(p) => p,
                None => return Response::err_code("bad_did_key", "not an Ed25519 did:key"),
            };
            let vk = match VerifyingKey::from_bytes(&pk) {
                Ok(v) => v,
                Err(e) => return Response::err_code("bad_did_key", format!("pubkey: {e}")),
            };
            Response::ok(serde_json::json!({ "valid": vk.verify(&payload, &sig).is_ok() }))
        }
        Request::Shutdown {} => {
            let mut guard = node.lock().await;
            *guard = None;
            Response::ok(serde_json::json!({ "message": "Provider shutting down" }))
        }
    }
}

/// Run `f` against an initialized node, or return a not_init error.
async fn with_node<F>(node: &tokio::sync::Mutex<Option<Node>>, f: F) -> Response
where
    F: FnOnce(&Node) -> Response,
{
    let guard = node.lock().await;
    match guard.as_ref() {
        Some(n) => f(n),
        None => Response::err_code("not_init", "init first"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ml_kem::kem::Encapsulate;

    fn master() -> [u8; 32] {
        [7u8; 32]
    }

    #[test]
    fn derivation_is_deterministic_and_per_principal() {
        let m = master();
        let a1 = derive_bundle(&m, "person:local:aaa", "hey");
        let a2 = derive_bundle(&m, "person:local:aaa", "hey");
        let b = derive_bundle(&m, "person:local:bbb", "hey");
        // Same principal+namespace → identical identity across calls/restarts.
        assert_eq!(a1.did_key(), a2.did_key());
        assert_eq!(a1.x25519_pub(), a2.x25519_pub());
        assert_eq!(a1.ml_ek_bytes, a2.ml_ek_bytes);
        // Different principal → different identity (no cross-user collision).
        assert_ne!(a1.did_key(), b.did_key());
        assert_ne!(a1.x25519_pub(), b.x25519_pub());
        assert_ne!(a1.ml_ek_bytes, b.ml_ek_bytes);
        assert!(a1.did_key().starts_with("did:key:z"));
    }

    #[test]
    fn ml_kem_encapsulate_then_provider_decapsulate_agree() {
        let b = derive_bundle(&master(), "person:local:aaa", "hey");
        // Sender side (peer): encapsulate to our advertised ML-KEM pubkey.
        let ek = <<MlKem768 as KemCore>::EncapsulationKey as EncodedSizeUser>::from_bytes(
            b.ml_ek_bytes.as_slice().try_into().unwrap(),
        );
        let (ct, shared_enc) = ek.encapsulate(&mut OsRng).unwrap();
        let ct_bytes: &[u8] = &ct;
        // Recipient side (provider): decapsulate must recover the same secret.
        let shared_dec_b64 = b.ml_kem_decapsulate(ct_bytes).unwrap();
        let shared_enc_bytes: &[u8] = &shared_enc;
        assert_eq!(B64.encode(shared_enc_bytes), shared_dec_b64);
    }

    #[test]
    fn x25519_dh_is_symmetric() {
        let b = derive_bundle(&master(), "person:local:aaa", "hey");
        let mut sb = [0u8; 32];
        OsRng.fill_bytes(&mut sb);
        let eph_secret = XSecret::from(sb);
        let eph_pub = XPub::from(&eph_secret).to_bytes();
        // Provider computes our_secret · eph_pub.
        let provider_shared = b.x25519_dh(eph_pub);
        // Peer computes eph_secret · our_pub → must match.
        let peer_shared = eph_secret.diffie_hellman(&XPub::from(b.x25519_pub()));
        assert_eq!(B64.encode(peer_shared.as_bytes()), provider_shared);
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let node: tokio::sync::Mutex<Option<Node>> = tokio::sync::Mutex::new(None);
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let mut stdout = tokio::io::stdout();

    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let resp = match serde_json::from_str::<Request>(trimmed) {
            Ok(req) => handle(&node, req).await,
            Err(e) => Response::err_code("invalid_request", format!("{e}")),
        };
        let mut out = serde_json::to_vec(&resp)?;
        out.push(b'\n');
        stdout.write_all(&out).await?;
        stdout.flush().await?;
    }
    Ok(())
}
