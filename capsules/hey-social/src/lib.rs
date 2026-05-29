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
    // Wallet-style boot — narrow scope on purpose:
    //   1. Redeem any ?home_token=... the runtime appended to our URL
    //      via redeem_launch_token() (POSTs to the canonical
    //      /api/apps/hey-social/session/start with x-elastos-home-token,
    //      falls back to /runtime-token on older runtimes; either way
    //      the runtime sets an HttpOnly app-scoped session cookie that
    //      every subsequent fetch carries via credentials: 'include').
    //   2. Scrub the launch token from the visible URL so it can't leak
    //      via screenshots, bookmarks, or browser history.
    //   3. Pre-warm capability tokens for declared providers.
    //
    // Inheriting the runtime's session (calling /api/session, bootstrapping
    // a thin localStorage Session) lives in Landing's Effect, NOT here.
    // That avoids a race: if inherit_session resolves AFTER Landing has
    // rendered, Landing's one-shot Effect won't re-fire on a localStorage
    // write and the user is stuck looking at a passkey CTA they shouldn't
    // need. Landing owning its own inherit means the navigate-away path
    // is the same reactive context as the render path.
    spawn_local(async {
        let redeemed = runtime::redeem_launch_token().await;
        runtime::boot_log(&format!("launch-token redeem -> {redeemed}"));
        runtime::scrub_launch_token_from_url();
        runtime::acquire_boot_capabilities().await;
        runtime::boot_log("boot capabilities acquired");
    });

    // Safety net: the boot splash is normally dismissed by Home (warp into
    // feed) or Landing (fade to the sign-in CTA). If the iframe was reloaded
    // on a sub-route (e.g. /profile) neither fires, so guarantee the splash
    // never sticks — a plain fade after a generous beat. No-op if already
    // dismissed by the nicer transition.
    spawn_local(async {
        runtime::sleep_ms(4000).await;
        runtime::hide_boot_splash();
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
