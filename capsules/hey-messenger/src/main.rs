use hey_chat::ctx::{init, CapsuleCtx};
use hey_messenger::App;

// Per-capsule identity for the shared hey-chat engine. The messenger uses
// its OWN capsule id, storage namespace, and localStorage/sessionStorage
// keys (separate per-app session — same DID via the passkey PRF, but an
// independent sign-in), and requests ONLY the providers it needs:
//   peer  — Carrier iroh-gossip transport (DMs)
//   blobs — iroh-blobs attachments
//   did   — did:key resolve
// Deliberately NOT content/social-feed/hey-transcoder/elacity (social-only).
const HEY_MESSENGER_CTX: CapsuleCtx = CapsuleCtx {
    capsule_id: "hey-messenger",
    private_namespace: "HeyMessenger",
    session_key: "hey-messenger-session",
    welcomed_key: "hey-messenger-welcomed",
    session_redeemed_key: "hey-messenger-redeemed",
    home_launch_token_key: "hey-messenger-home-token",
    runtime_token_key: "hey-messenger-runtime-token",
    token_store_key: "hey-messenger-capability-tokens",
    route_mode_key: "hey-messenger-storage-route-mode",
    boot_capabilities: &[
        ("elastos://peer/*", "message"),
        ("elastos://blobs/*", "write"),
        ("elastos://did/*", "read"),
    ],
};

fn main() {
    console_error_panic_hook::set_once();
    // MUST run before any engine call — App's boot tasks read the ctx.
    init(HEY_MESSENGER_CTX);
    leptos::mount::mount_to_body(App);
}
