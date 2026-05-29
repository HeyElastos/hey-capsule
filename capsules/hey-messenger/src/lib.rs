use std::borrow::Cow;

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;

// Derive the router base from the iframe mount path. Under YunoHost the
// capsule loads at e.g. `/apps/hey-messenger/` — without this the Router
// sees the full pathname and matches nothing. Same heuristic as hey-social.
fn router_base() -> Cow<'static, str> {
    (|| -> Option<String> {
        let win = web_sys::window()?;
        let path = win.location().pathname().ok()?;
        let idx = path.find("/apps/")?;
        let after = &path[idx + 6..];
        let end = after.find('/').map(|j| idx + 6 + j).unwrap_or(path.len());
        Some(path[..end].to_string())
    })()
    .map(Cow::Owned)
    .unwrap_or(Cow::Borrowed(""))
}

#[component]
pub fn App() -> impl IntoView {
    // Boot against the shared engine (ctx::init already ran in main):
    //   1. redeem any ?home_token=… into an app-scoped session,
    //   2. scrub the token from the visible URL,
    //   3. pre-warm the capability tokens this capsule declared,
    //   4. start the chat receive loop (no-op while signed out).
    spawn_local(async {
        let _ = hey_chat::runtime::redeem_launch_token().await;
        hey_chat::runtime::scrub_launch_token_from_url();
        hey_chat::runtime::acquire_boot_capabilities().await;
    });
    spawn_local(async {
        hey_chat::peer_receiver::run().await;
    });

    let base = router_base();
    view! {
        <Router base=base>
            <Routes fallback=|| view! { <p>"Not found"</p> }>
                <Route path=path!("/") view=Shell />
                <Route path=path!("/chat/:did") view=Shell />
            </Routes>
        </Router>
    }
}

/// Telegram-desktop 2-pane shell (chat list │ conversation). Placeholder —
/// the real ChatList / Conversation / Composer / SignInGate screens land
/// next; this compiles + boots the engine so the foundation is verifiable.
#[component]
fn Shell() -> impl IntoView {
    view! {
        <div class="flex h-screen w-screen text-zinc-900 dark:text-zinc-100">
            <aside class="w-72 shrink-0 border-r border-zinc-200 dark:border-zinc-800 p-4">
                <h1 class="text-lg font-semibold">"Hey Chat"</h1>
                <p class="mt-2 text-sm text-zinc-500">"Conversations will appear here."</p>
            </aside>
            <section class="flex-1 grid place-items-center">
                <p class="text-zinc-500">
                    "Telegram-desktop UI in progress — select a conversation."
                </p>
            </section>
        </div>
    }
}
