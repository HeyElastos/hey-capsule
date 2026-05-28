use std::borrow::Cow;

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;

pub mod api;
pub mod app_modals;
pub mod components;
pub mod crypto;
pub mod events;
pub mod identity;
pub mod ipld;
pub mod pages;
pub mod passkey;
pub mod peer_receiver;
pub mod runtime;
pub mod session;
pub mod shell;

// Derive the router base from the iframe's mount path. Under YunoHost the
// capsule loads at e.g. `/apps/hey-social/` (or `/<prefix>/apps/.../`
// when behind a subpath) — without this, the Leptos Router sees the full
// URL pathname and can't match any route, falling through to the NotFound
// branch. Mirrors the React reference's BrowserRouter `basename` heuristic.
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
    // Pre-warm capability tokens for the providers we'll touch. The runtime
    // auto-grants any resource declared in capsule.json, so this is one
    // round-trip per provider on first launch and zero on every subsequent
    // navigation (cached in sessionStorage).
    spawn_local(async {
        runtime::acquire_boot_capabilities().await;
    });

    // Start the peer-receive subscription loop. No-op while signed out;
    // begins polling per-topic the moment a session appears.
    spawn_local(async {
        peer_receiver::run().await;
    });

    let modals = app_modals::AppModals::default();
    provide_context(modals);

    let base = router_base();

    view! {
        <Router base=base>
            <main class="min-h-screen text-primary">
                <Routes fallback=|| view! { <pages::NotFound /> }>
                    <Route path=path!("/") view=pages::Home />
                    <Route path=path!("/videos") view=pages::Clips />
                    <Route path=path!("/posts") view=pages::Posts />
                    <Route path=path!("/p/:id") view=pages::PostDetail />
                    <Route path=path!("/v/:id") view=pages::VideoPlayer />
                    <Route path=path!("/profile") view=pages::Profile />
                    <Route path=path!("/profile/:did") view=pages::Profile />
                    <Route path=path!("/chat") view=pages::Chat />
                    <Route path=path!("/chat/g/:group_id") view=pages::Chat />
                    <Route path=path!("/chat/:did") view=pages::Chat />
                    <Route path=path!("/welcome") view=pages::Onboarding />
                    <Route path=path!("/signup") view=pages::SignUp />
                    <Route path=path!("/signin") view=pages::SignIn />
                    // Backwards-compat aliases:
                    <Route path=path!("/home") view=pages::Home />
                    <Route path=path!("/clips") view=pages::Clips />
                    <Route path=path!("/post/:id") view=pages::PostDetail />
                    <Route path=path!("/video/:id") view=pages::VideoPlayer />
                    <Route path=path!("/onboarding") view=pages::Onboarding />
                </Routes>
                <components::NotificationPanel open=modals.notifications_open />
                <components::SearchModal open=modals.search_open />
                <components::AddFriendModal open=modals.add_friend_open />
                <components::NewGroupModal open=modals.new_group_open />
            </main>
        </Router>
    }
}
