//! peer-provider — iroh-gossip pub/sub for Hey.
//!
//! Wire protocol mirrors blobs-provider: line-delimited JSON on stdin
//! (one Request per line), line-delimited JSON on stdout. The runtime
//! spawns this as a subprocess and dispatches `elastos://peer/*`
//! provider calls to it.
//!
//! Operations:
//!   init                                       -> { node_id, ticket }
//!   gossip_join     { topic }                  -> { ok, peer_count }
//!   gossip_leave    { topic }                  -> { ok }
//!   gossip_send     { topic, message,
//!                     sender_id, ts, signature } -> { ok, seq }
//!   gossip_recv     { topic, limit,
//!                     consumer_id,
//!                     skip_sender_id? }        -> { messages: [...] }
//!   list_topic_peers{ topic }                  -> { peers: [<node_id>] }
//!   list_peers                                 -> { peers: [<node_id>] }
//!   get_ticket                                 -> { ticket }
//!
//! Topic naming is opaque — the provider hashes whatever string the
//! capsule sends to a 32-byte blake3 digest before handing it to
//! iroh-gossip. So the capsule is free to use random queue ids
//! ("q/<256bit-hex>"), did-pair hashes, or anything else; the
//! provider doesn't know or care.
//!
//! Storage:
//!   $XDG_DATA_HOME/elastos/peer-provider/
//!     secret.key           — iroh endpoint secret (stable node id)
//!     topics.json          — joined topics persisted across restarts
//!     cursors.json         — per (topic, consumer_id) read cursor

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use bytes::Bytes;
use futures_util::StreamExt;
use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr, EndpointId, SecretKey};
use iroh_gossip::api::{Event, GossipReceiver, GossipSender};
use iroh_gossip::net::{Gossip, GOSSIP_ALPN};
use iroh_gossip::proto::TopicId;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// How many messages to keep per topic in the per-topic ring buffer.
/// Older messages are evicted FIFO. Sized so a 5-second poll window
/// can't realistically overrun even on a busy group topic.
const PER_TOPIC_BUFFER_CAP: usize = 1024;

// ── Wire protocol (matches the runtime's ProviderResponse) ───────────

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    Init {},
    GossipJoin {
        topic: String,
    },
    GossipLeave {
        topic: String,
    },
    GossipSend {
        topic: String,
        message: String,
        sender_id: String,
        #[serde(default)]
        ts: i64,
        #[serde(default)]
        signature: String,
    },
    GossipRecv {
        topic: String,
        #[serde(default = "default_limit")]
        limit: u32,
        consumer_id: String,
        #[serde(default)]
        skip_sender_id: Option<String>,
    },
    ListTopicPeers {
        topic: String,
    },
    ListPeers {},
    GetTicket {},
}

fn default_limit() -> u32 {
    50
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
    fn err(msg: impl Into<String>) -> Self {
        Self::Error {
            code: "peer_provider".into(),
            message: msg.into(),
        }
    }
}

// ── Buffered message — what gossip_recv returns ──────────────────────

#[derive(Debug, Clone, Serialize)]
struct StoredMessage {
    /// Monotonically increasing per topic. Used as the cursor key.
    seq: u64,
    message: String,
    sender_id: String,
    ts: i64,
    signature: String,
}

#[derive(Debug, Default)]
struct TopicBuffer {
    /// Ring buffer of recent messages on this topic. Newest pushed at
    /// the back; oldest popped at the front when capacity is reached.
    items: VecDeque<StoredMessage>,
    /// Monotonic sequence counter — never resets so old cursors stay
    /// usable until the message ages out.
    next_seq: u64,
    /// Peer NodeIds currently subscribed alongside us (best-effort).
    peers: Vec<EndpointId>,
}

#[derive(Debug, Default)]
struct State {
    buffers: HashMap<TopicId, TopicBuffer>,
    /// Per (topic_id, consumer_id) → highest seq already returned.
    cursors: HashMap<(TopicId, String), u64>,
    /// String -> TopicId mapping cache (we hash with blake3 to
    /// derive the 32-byte topic id from arbitrary topic strings).
    topic_ids: HashMap<String, TopicId>,
    /// Background subscription tasks per joined topic. Holding the
    /// JoinHandle keeps the task alive; dropping it on leave aborts.
    senders: HashMap<TopicId, GossipSender>,
    handles: HashMap<TopicId, tokio::task::JoinHandle<()>>,
}

// ── Node — iroh endpoint + gossip protocol ───────────────────────────

struct Node {
    endpoint: Endpoint,
    gossip: Gossip,
    state: Arc<Mutex<State>>,
    data_dir: PathBuf,
}

impl Node {
    async fn spawn(data_dir: PathBuf) -> Result<Self> {
        tokio::fs::create_dir_all(&data_dir)
            .await
            .context("create data dir")?;
        let secret = load_or_create_secret(&data_dir).await?;
        // presets::N0 enables n0's relay + DNS discovery so the
        // EndpointId in our tickets is reachable from the public
        // network without manual NAT punching.
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(secret)
            .bind()
            .await?;
        let gossip = Gossip::builder().spawn(endpoint.clone());
        // Wire GOSSIP_ALPN onto the endpoint so incoming gossip
        // connections from peers actually land. We don't drop the
        // router (it would shut down acceptance) — leak it.
        let router = iroh::protocol::Router::builder(endpoint.clone())
            .accept(GOSSIP_ALPN, gossip.clone())
            .spawn();
        let _ = Box::leak(Box::new(router));

        let state = Arc::new(Mutex::new(State::default()));
        Ok(Self {
            endpoint,
            gossip,
            state,
            data_dir,
        })
    }

    fn topic_id_for(&self, topic_str: &str) -> TopicId {
        {
            let st = self.state.lock();
            if let Some(t) = st.topic_ids.get(topic_str) {
                return *t;
            }
        }
        let hash = blake3::hash(topic_str.as_bytes());
        let id = TopicId::from_bytes(*hash.as_bytes());
        self.state
            .lock()
            .topic_ids
            .insert(topic_str.to_string(), id);
        id
    }

    fn node_ticket(&self) -> Result<String> {
        // EndpointAddr is the serializable peer-address shape; peers
        // recovering us from a ticket parse it back through serde_json.
        let addr = EndpointAddr::from(self.endpoint.id());
        Ok(serde_json::to_string(&addr)?)
    }

    async fn gossip_join(&self, topic_str: &str) -> Result<u64> {
        let topic_id = self.topic_id_for(topic_str);
        if self.state.lock().handles.contains_key(&topic_id) {
            // Already subscribed.
            let peer_count = self
                .state
                .lock()
                .buffers
                .get(&topic_id)
                .map(|b| b.peers.len() as u64)
                .unwrap_or(0);
            return Ok(peer_count);
        }
        // bootstrap = empty; rely on n0 discovery + future peer
        // introductions via list_topic_peers.
        let topic = self
            .gossip
            .subscribe(topic_id, vec![])
            .await
            .context("gossip subscribe")?;
        let (sender, receiver) = topic.split();
        let state = self.state.clone();
        let handle = tokio::spawn(consume_gossip(topic_id, receiver, state.clone()));
        let mut st = self.state.lock();
        st.senders.insert(topic_id, sender);
        st.handles.insert(topic_id, handle);
        st.buffers.entry(topic_id).or_default();
        Ok(0)
    }

    async fn gossip_leave(&self, topic_str: &str) -> Result<()> {
        let topic_id = self.topic_id_for(topic_str);
        let (sender, handle) = {
            let mut st = self.state.lock();
            (st.senders.remove(&topic_id), st.handles.remove(&topic_id))
        };
        drop(sender);
        if let Some(h) = handle {
            h.abort();
        }
        // Keep the buffer + cursors so a quick rejoin sees history.
        // A full identity wipe would clear data_dir.
        Ok(())
    }

    async fn gossip_send(
        &self,
        topic_str: &str,
        message: String,
        sender_id: String,
        ts: i64,
        signature: String,
    ) -> Result<u64> {
        let topic_id = self.topic_id_for(topic_str);
        // Lazy-join if the capsule sends without joining first.
        if !self.state.lock().senders.contains_key(&topic_id) {
            self.gossip_join(topic_str).await?;
        }
        let envelope = WireEnvelope {
            sender_id: sender_id.clone(),
            ts,
            signature: signature.clone(),
            payload: message.clone(),
        };
        let bytes = serde_json::to_vec(&envelope).context("envelope serialize")?;
        let mut sender = self
            .state
            .lock()
            .senders
            .get(&topic_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("not joined"))?;
        sender
            .broadcast(Bytes::from(bytes))
            .await
            .context("gossip broadcast")?;
        // Also append to our own buffer so a same-runtime second
        // consumer (e.g. another tab) sees it without round-tripping
        // through the gossip network.
        let mut st = self.state.lock();
        let buf = st.buffers.entry(topic_id).or_default();
        let seq = buf.next_seq;
        buf.next_seq += 1;
        buf.items.push_back(StoredMessage {
            seq,
            message,
            sender_id,
            ts,
            signature,
        });
        while buf.items.len() > PER_TOPIC_BUFFER_CAP {
            buf.items.pop_front();
        }
        Ok(seq)
    }

    fn gossip_recv(
        &self,
        topic_str: &str,
        limit: u32,
        consumer_id: &str,
        skip_sender_id: Option<&str>,
    ) -> Vec<StoredMessage> {
        let topic_id = self.topic_id_for(topic_str);
        let mut st = self.state.lock();
        let cursor_key = (topic_id, consumer_id.to_string());
        let cursor = st.cursors.get(&cursor_key).copied().unwrap_or(0);
        let buf = match st.buffers.get(&topic_id) {
            Some(b) => b,
            None => return Vec::new(),
        };
        let mut out = Vec::new();
        let mut new_cursor = cursor;
        for item in buf.items.iter() {
            if item.seq < cursor {
                continue;
            }
            if let Some(skip) = skip_sender_id {
                if item.sender_id == skip {
                    new_cursor = new_cursor.max(item.seq + 1);
                    continue;
                }
            }
            out.push(item.clone());
            new_cursor = item.seq + 1;
            if out.len() as u32 >= limit {
                break;
            }
        }
        st.cursors.insert(cursor_key, new_cursor);
        out
    }

    fn list_topic_peers(&self, topic_str: &str) -> Vec<String> {
        let topic_id = self.topic_id_for(topic_str);
        self.state
            .lock()
            .buffers
            .get(&topic_id)
            .map(|b| b.peers.iter().map(|p| p.to_string()).collect())
            .unwrap_or_default()
    }

    fn list_peers(&self) -> Vec<String> {
        let st = self.state.lock();
        let mut set = std::collections::HashSet::new();
        for buf in st.buffers.values() {
            for p in &buf.peers {
                set.insert(p.to_string());
            }
        }
        set.into_iter().collect()
    }
}

/// Envelope we put on the wire for each broadcast. Carries the same
/// fields the capsule supplied so the receiver can reconstruct the
/// caller-visible shape of `gossip_recv`. payload is the opaque blob
/// — encrypted + signed by the capsule, the provider never reads it.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct WireEnvelope {
    sender_id: String,
    ts: i64,
    signature: String,
    payload: String,
}

/// Background task: poll one topic's GossipReceiver and stuff each
/// incoming message into the shared buffer. Tracks peer membership
/// from join/leave events so list_topic_peers stays current.
async fn consume_gossip(topic_id: TopicId, mut receiver: GossipReceiver, state: Arc<Mutex<State>>) {
    while let Some(event) = receiver.next().await {
        let Ok(event) = event else {
            // Stream error: keep going until close.
            continue;
        };
        match event {
            Event::Received(msg) => {
                let env: WireEnvelope = match serde_json::from_slice(&msg.content) {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::warn!(error=%e, "drop non-envelope gossip message");
                        continue;
                    }
                };
                let mut st = state.lock();
                let buf = st.buffers.entry(topic_id).or_default();
                let seq = buf.next_seq;
                buf.next_seq += 1;
                buf.items.push_back(StoredMessage {
                    seq,
                    message: env.payload,
                    sender_id: env.sender_id,
                    ts: env.ts,
                    signature: env.signature,
                });
                while buf.items.len() > PER_TOPIC_BUFFER_CAP {
                    buf.items.pop_front();
                }
            }
            Event::NeighborUp(node_id) => {
                let mut st = state.lock();
                let buf = st.buffers.entry(topic_id).or_default();
                if !buf.peers.contains(&node_id) {
                    buf.peers.push(node_id);
                }
            }
            Event::NeighborDown(node_id) => {
                let mut st = state.lock();
                let buf = st.buffers.entry(topic_id).or_default();
                buf.peers.retain(|p| *p != node_id);
            }
            _ => {}
        }
    }
}

// ── Persistence (just the secret key for now) ────────────────────────

async fn load_or_create_secret(data_dir: &PathBuf) -> Result<SecretKey> {
    let path = data_dir.join("secret.key");
    if let Ok(bytes) = tokio::fs::read(&path).await {
        let decoded = B64.decode(&bytes).context("decode secret")?;
        let arr: [u8; 32] = decoded
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("secret key wrong size"))?;
        return Ok(SecretKey::from_bytes(&arr));
    }
    let secret = SecretKey::generate();
    let encoded = B64.encode(secret.to_bytes());
    tokio::fs::write(&path, encoded.as_bytes())
        .await
        .context("write secret")?;
    Ok(secret)
}

fn data_dir() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".local/share")
        });
    base.join("elastos/peer-provider")
}

// ── Dispatch ─────────────────────────────────────────────────────────

async fn handle(node: &tokio::sync::Mutex<Option<Node>>, req: Request) -> Response {
    match req {
        Request::Init {} => {
            let mut guard = node.lock().await;
            if guard.is_some() {
                return Response::ok(serde_json::json!({ "already_initialized": true }));
            }
            match Node::spawn(data_dir()).await {
                Ok(n) => {
                    let node_id = n.endpoint.id().to_string();
                    let ticket = n.node_ticket().unwrap_or_default();
                    *guard = Some(n);
                    Response::ok(serde_json::json!({
                        "node_id": node_id,
                        "ticket": ticket,
                    }))
                }
                Err(e) => Response::err(format!("init failed: {e:#}")),
            }
        }
        Request::GossipJoin { topic } => {
            let guard = node.lock().await;
            let Some(n) = guard.as_ref() else {
                return Response::err("not initialized");
            };
            match n.gossip_join(&topic).await {
                Ok(peer_count) => Response::ok(serde_json::json!({
                    "ok": true,
                    "peer_count": peer_count,
                })),
                Err(e) => Response::err(format!("join failed: {e:#}")),
            }
        }
        Request::GossipLeave { topic } => {
            let guard = node.lock().await;
            let Some(n) = guard.as_ref() else {
                return Response::err("not initialized");
            };
            match n.gossip_leave(&topic).await {
                Ok(_) => Response::ok(serde_json::json!({ "ok": true })),
                Err(e) => Response::err(format!("leave failed: {e:#}")),
            }
        }
        Request::GossipSend {
            topic,
            message,
            sender_id,
            ts,
            signature,
        } => {
            let guard = node.lock().await;
            let Some(n) = guard.as_ref() else {
                return Response::err("not initialized");
            };
            match n
                .gossip_send(&topic, message, sender_id, ts, signature)
                .await
            {
                Ok(seq) => Response::ok(serde_json::json!({ "ok": true, "seq": seq })),
                Err(e) => Response::err(format!("send failed: {e:#}")),
            }
        }
        Request::GossipRecv {
            topic,
            limit,
            consumer_id,
            skip_sender_id,
        } => {
            let guard = node.lock().await;
            let Some(n) = guard.as_ref() else {
                return Response::err("not initialized");
            };
            let msgs = n.gossip_recv(&topic, limit, &consumer_id, skip_sender_id.as_deref());
            Response::ok(serde_json::json!({ "messages": msgs }))
        }
        Request::ListTopicPeers { topic } => {
            let guard = node.lock().await;
            let Some(n) = guard.as_ref() else {
                return Response::err("not initialized");
            };
            Response::ok(serde_json::json!({ "peers": n.list_topic_peers(&topic) }))
        }
        Request::ListPeers {} => {
            let guard = node.lock().await;
            let Some(n) = guard.as_ref() else {
                return Response::err("not initialized");
            };
            Response::ok(serde_json::json!({ "peers": n.list_peers() }))
        }
        Request::GetTicket {} => {
            let guard = node.lock().await;
            let Some(n) = guard.as_ref() else {
                return Response::err("not initialized");
            };
            match n.node_ticket() {
                Ok(t) => Response::ok(serde_json::json!({ "ticket": t })),
                Err(e) => Response::err(format!("ticket failed: {e:#}")),
            }
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
            Err(e) => Response::err(format!("invalid request: {e}")),
        };
        let mut out = serde_json::to_vec(&resp)?;
        out.push(b'\n');
        stdout.write_all(&out).await?;
        stdout.flush().await?;
    }
    Ok(())
}
