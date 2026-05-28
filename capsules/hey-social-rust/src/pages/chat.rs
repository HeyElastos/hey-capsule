// Chat — currently a placeholder. The React reference at
// capsules/hey-social/client/src/pages/Chat.jsx is 1428 lines of E2E
// crypto (ML-KEM-768 + X25519 hybrid via @noble/post-quantum) on top of
// peer.publish/recv. Porting it requires:
//   - ML-KEM-768 implementation in Rust (no current crate works in wasm
//     without significant code-size cost — pqcrypto-mlkem needs verification).
//   - X25519 (have ed25519-compact, no x25519 yet).
//   - Group key exchange + ratchet state machine.
//
// For now we point users at Hey Messenger (the standalone messaging
// capsule that already has the E2E layer in JS) and document the gap.
// Pull-request welcome — the protocol spec lives in the React file.

use leptos::prelude::*;

use crate::components::{FloatingDock, TopHeader};

#[component]
pub fn Chat() -> impl IntoView {
    view! {
        <>
            <TopHeader />
            <FloatingDock />
            <div class="mx-auto max-w-2xl px-4 py-10 sm:px-6">
                <div class="frosted-card p-8 text-center animate-fade-up">
                    <h2 class="logo-handwritten text-4xl text-primary">
                        "Chat lives in Hey Messenger"
                    </h2>
                    <p class="mt-3 text-sm text-muted max-w-md mx-auto">
                        "End-to-end-encrypted messaging in this Rust port is still being ported (the React reference uses ML-KEM-768 + X25519 hybrid post-quantum crypto — that lives in 1400 lines of JS). Use the standalone Hey Messenger capsule, same passkey, same DID."
                    </p>
                    // Direct browser href — hey-messenger is a SEPARATE
                    // capsule, not an internal route, so no NavLink/Router.
                    <a
                        href="../hey-messenger/"
                        class="unfrost mt-6 inline-flex items-center gap-2 rounded-full bg-accent px-6 py-2.5 text-sm font-semibold text-accent-text shadow-md transition hover:bg-amber-300"
                    >
                        "Open Hey Messenger"
                    </a>
                </div>
            </div>
        </>
    }
}
