// FloatingDock — left-side vertical column with all primary actions.
// Always on the side (not responsive bottom bar) so the chrome stays
// consistent across widths. Includes:
//   * Feed / Posts(+) / Chat / Profile  — page navigation
//   * Search / Add-friend / Bell        — global modals (toggled via
//                                         AppModals context)
//
// Active route gets the .is-active accent glow (defined in styles.css).
// The bell shows a live unread badge that the peer_receiver feeds via
// api::notifications::unread_count.

use leptos::ev::MouseEvent;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_location;

use crate::api::notifications;
use crate::app_modals::AppModals;
use crate::components::icons::{
    BellIcon, ChatIcon, HomeIcon, PlusIcon, SearchIcon, UserIcon,
};
use crate::components::NavLink;
use crate::session;

#[component]
pub fn FloatingDock() -> impl IntoView {
    let location = use_location();
    let modals = use_context::<AppModals>().unwrap_or_default();
    let notifications_open = modals.notifications_open;
    let search_open = modals.search_open;
    let add_friend_open = modals.add_friend_open;
    let dock_open = modals.dock_open;

    let active = move |p: &str| -> bool {
        let path = location.pathname.get();
        match p {
            "/" => path == "/" || path == "/home",
            "/posts" => path == "/posts",
            "/chat" => path.starts_with("/chat"),
            "/profile" => path.starts_with("/profile"),
            _ => path == p,
        }
    };

    let icon_class = move |is_active: bool| -> String {
        if is_active {
            "icon-btn is-active h-12 w-12 inline-flex items-center justify-center".into()
        } else {
            "icon-btn h-12 w-12 inline-flex items-center justify-center".into()
        }
    };

    // Live unread count for the bell badge.
    let unread = RwSignal::new(0usize);
    Effect::new(move |_| {
        spawn_local(async move {
            loop {
                if session::current().is_some() {
                    let n = notifications::unread_count().await;
                    unread.set(n);
                }
                wait_10s().await;
            }
        });
    });

    let toggle_dock = move |_: MouseEvent| dock_open.update(|v| *v = !*v);

    view! {
        <aside class="
            fixed z-40 left-3 top-1/2 -translate-y-1/2
            sm:left-4
            flex items-center
        ">
            // Collapsed state: just the chevron tab. Tap to expand.
            {move || if !dock_open.get() {
                view! {
                    <button
                        type="button"
                        on:click=toggle_dock
                        class="frosted-card p-2 inline-flex items-center justify-center"
                        aria-label="Open dock"
                        title="Open dock"
                    >
                        <svg viewBox="0 0 24 24" class="h-5 w-5 text-primary" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <path d="m9 18 6-6-6-6" />
                        </svg>
                    </button>
                }.into_any()
            } else {
                view! { <></> }.into_any()
            }}

            // Expanded state: full dock + chevron-left tab on the right edge to collapse.
            {move || if dock_open.get() {
                view! {
                    <div class="frosted-card p-0 w-16 sm:w-20 relative">
                <nav class="flex flex-col items-stretch gap-1 p-2">
                    <NavLink
                        href="/"
                        class=icon_class(active("/"))
                        title="Feed"
                        aria_label="Feed"
                    >
                        <HomeIcon class="h-6 w-6" />
                    </NavLink>
                    <NavLink
                        href="/posts"
                        class=icon_class(active("/posts"))
                        title="New post"
                        aria_label="New post"
                    >
                        <PlusIcon class="h-6 w-6" />
                    </NavLink>
                    <NavLink
                        href="/chat"
                        class=icon_class(active("/chat"))
                        title="Chat"
                        aria_label="Chat"
                    >
                        <ChatIcon class="h-6 w-6" />
                    </NavLink>
                    <NavLink
                        href="/profile"
                        class=icon_class(active("/profile"))
                        title="Profile"
                        aria_label="Profile"
                    >
                        <UserIcon class="h-6 w-6" />
                    </NavLink>

                    <div class="my-1 h-px bg-white/15 mx-2" />

                    <button
                        type="button"
                        on:click=move |_: MouseEvent| search_open.set(true)
                        class="icon-btn h-12 w-12 inline-flex items-center justify-center mx-auto"
                        title="Find user"
                        aria-label="Find user"
                    >
                        <SearchIcon class="h-6 w-6" />
                    </button>
                    <button
                        type="button"
                        on:click=move |_: MouseEvent| add_friend_open.set(true)
                        class="icon-btn h-12 w-12 inline-flex items-center justify-center mx-auto"
                        title="Add friend"
                        aria-label="Add friend"
                    >
                        <PlusIcon class="h-6 w-6" />
                    </button>
                    <button
                        type="button"
                        on:click=move |_: MouseEvent| notifications_open.set(true)
                        class="icon-btn h-12 w-12 inline-flex items-center justify-center mx-auto relative"
                        title="Notifications"
                        aria-label="Notifications"
                    >
                        <BellIcon class="h-6 w-6" />
                        {move || {
                            let n = unread.get();
                            if n == 0 { view! { <></> }.into_any() } else {
                                let label = if n > 9 { "9+".to_string() } else { n.to_string() };
                                view! {
                                    <span class="pointer-events-none absolute -right-0.5 -top-0.5 flex h-4 min-w-4 items-center justify-center rounded-full bg-rose-500 px-1 text-[10px] font-bold leading-none text-white">
                                        {label}
                                    </span>
                                }.into_any()
                            }
                        }}
                    </button>
                </nav>

                    // Chevron-left tab that pokes out the right edge.
                    <button
                        type="button"
                        on:click=toggle_dock
                        class="
                            absolute top-1/2 -translate-y-1/2 -right-3
                            frosted-card p-1 inline-flex items-center justify-center
                            !rounded-full
                        "
                        aria-label="Hide dock"
                        title="Hide dock"
                    >
                        <svg viewBox="0 0 24 24" class="h-4 w-4 text-primary" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <path d="m15 18-6-6 6-6" />
                        </svg>
                    </button>
                </div>
                }.into_any()
            } else {
                view! { <></> }.into_any()
            }}
        </aside>
    }
}

async fn wait_10s() {
    let win = web_sys::window().unwrap();
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let _ = win
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 10_000);
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}
