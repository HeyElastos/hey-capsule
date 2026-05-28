//! identity-projection-provider — capsules ask "who am I" and "sign
//! this" instead of holding the Ed25519 seed in localStorage.
//!
//! Today the seed is per-capsule (derived from the passkey PRF inside
//! the WASM bundle, stashed in localStorage). That leaks: any XSS in
//! the bundle reads the seed; logout doesn't actually clear it; the
//! shape forces every capsule to redo passkey + PRF + did:key
//! derivation. This provider does it once.
//!
//! Future direction: derive the projected key from a runtime-supplied
//! principal handle (so two different capsules talking to the same
//! provider get the same DID). For now we derive from a per-install
//! random secret + a per-namespace tag, which gives us:
//!   - same capsule on this device → same did:key across restarts
//!   - different capsule asking for a different namespace → different
//!     did:key (intentional; cross-capsule continuity is opt-in)
//!
//! Wire protocol mirrors blobs-provider / peer-provider: line-delimited
//! JSON on stdin/stdout. ProviderResponse-shaped responses.
//!
//! Operations:
//!   init                                  -> { protocol_version,
//!                                              provider, features }
//!   whoami    { namespace? }              -> { did_key, public_key_hex }
//!   sign      { namespace?, payload_b64 } -> { signature_hex }
//!   verify    { did_key, payload_b64,
//!               signature_hex }           -> { valid }
//!   shutdown                              -> ok
//!
//! Storage:
//!   $XDG_DATA_HOME/elastos/identity-projection-provider/
//!     master.key  — 32-byte random; HKDF'd per-namespace to derive
//!                   each did:key. Never exported.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use parking_lot::Mutex;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const DEFAULT_NAMESPACE: &str = "default";
const ED25519_PUB_MULTICODEC: [u8; 2] = [0xed, 0x01];

// ── Wire ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    Init {},
    Whoami {
        #[serde(default)]
        namespace: Option<String>,
    },
    Sign {
        #[serde(default)]
        namespace: Option<String>,
        payload_b64: String,
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

// ── Node ─────────────────────────────────────────────────────────────

struct Node {
    master_secret: [u8; 32],
    /// Cache of namespace → SigningKey so we don't re-derive on every
    /// sign call. Cleared on shutdown.
    cache: Arc<Mutex<HashMap<String, SigningKey>>>,
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

    /// HKDF-like per-namespace derivation. We use SHA-256 of
    /// (master_secret || namespace_bytes) as the seed for the Ed25519
    /// keypair. Two different namespaces yield independent keys; the
    /// same namespace on the same device yields the same key forever.
    fn signing_key(&self, namespace: &str) -> SigningKey {
        if let Some(k) = self.cache.lock().get(namespace) {
            return k.clone();
        }
        let mut hasher = Sha256::new();
        hasher.update(self.master_secret);
        hasher.update(b"|");
        hasher.update(namespace.as_bytes());
        let seed: [u8; 32] = hasher.finalize().into();
        let key = SigningKey::from_bytes(&seed);
        self.cache
            .lock()
            .insert(namespace.to_string(), key.clone());
        key
    }

    fn whoami(&self, namespace: &str) -> (String, String) {
        let key = self.signing_key(namespace);
        let pub_key = key.verifying_key();
        (
            public_key_to_did_key(pub_key.as_bytes()),
            hex::encode(pub_key.as_bytes()),
        )
    }

    fn sign(&self, namespace: &str, payload: &[u8]) -> String {
        let key = self.signing_key(namespace);
        let sig: Signature = key.sign(payload);
        hex::encode(sig.to_bytes())
    }
}

/// `did:key:z<base58btc(multicodec_ed25519_pub_prefix || pubkey)>`.
/// Matches the WC CCG spec + the capsule-side derivation in
/// hey-social's identity.rs.
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
    // Lock down the file to user-readable. Best-effort — not all
    // platforms support mode bits. Failures here are logged not fatal.
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

async fn handle(node: &tokio::sync::Mutex<Option<Node>>, req: Request) -> Response {
    match req {
        Request::Init {} => {
            let mut guard = node.lock().await;
            if guard.is_none() {
                match Node::spawn(data_dir()).await {
                    Ok(n) => *guard = Some(n),
                    Err(e) => {
                        return Response::err_code("init_failed", format!("{e:#}"));
                    }
                }
            }
            Response::ok(serde_json::json!({
                "protocol_version": "0.1",
                "provider": "identity",
                "features": ["whoami", "sign", "verify"],
            }))
        }
        Request::Whoami { namespace } => {
            let guard = node.lock().await;
            let Some(n) = guard.as_ref() else {
                return Response::err_code("not_init", "init first");
            };
            let ns = namespace.as_deref().unwrap_or(DEFAULT_NAMESPACE);
            let (did_key, pub_hex) = n.whoami(ns);
            Response::ok(serde_json::json!({
                "did_key": did_key,
                "public_key_hex": pub_hex,
                "namespace": ns,
            }))
        }
        Request::Sign {
            namespace,
            payload_b64,
        } => {
            let guard = node.lock().await;
            let Some(n) = guard.as_ref() else {
                return Response::err_code("not_init", "init first");
            };
            let ns = namespace.as_deref().unwrap_or(DEFAULT_NAMESPACE);
            let payload = match B64.decode(&payload_b64) {
                Ok(p) => p,
                Err(e) => {
                    return Response::err_code("bad_payload", format!("base64: {e}"));
                }
            };
            let sig = n.sign(ns, &payload);
            Response::ok(serde_json::json!({
                "signature_hex": sig,
                "namespace": ns,
            }))
        }
        Request::Verify {
            did_key,
            payload_b64,
            signature_hex,
        } => {
            let payload = match B64.decode(&payload_b64) {
                Ok(p) => p,
                Err(e) => {
                    return Response::err_code("bad_payload", format!("base64: {e}"));
                }
            };
            let sig_bytes = match hex::decode(&signature_hex) {
                Ok(b) => b,
                Err(e) => {
                    return Response::err_code("bad_sig", format!("hex: {e}"));
                }
            };
            let sig = match Signature::from_slice(&sig_bytes) {
                Ok(s) => s,
                Err(e) => {
                    return Response::err_code("bad_sig", format!("sig parse: {e}"));
                }
            };
            let pk = match did_key_to_public_key(&did_key) {
                Some(p) => p,
                None => {
                    return Response::err_code("bad_did_key", "not an Ed25519 did:key");
                }
            };
            let vk = match VerifyingKey::from_bytes(&pk) {
                Ok(v) => v,
                Err(e) => {
                    return Response::err_code("bad_did_key", format!("pubkey: {e}"));
                }
            };
            let valid = vk.verify(&payload, &sig).is_ok();
            Response::ok(serde_json::json!({ "valid": valid }))
        }
        Request::Shutdown {} => {
            let mut guard = node.lock().await;
            *guard = None;
            Response::ok(serde_json::json!({ "message": "Provider shutting down" }))
        }
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
