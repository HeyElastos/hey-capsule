// FloatingDock — left-side vertical column, frosted glass, with icon-btn
// links to feed / new-post / chat / profile. Uses NavLink instead of <A>
// so the Router base is applied to absolute hrefs even inside an iframe
// sandbox where <A>'s click interceptor doesn't fire reliably.

use leptos::prelude::*;

use crate::components::icons::{ChatIcon, HomeIcon, PlusIcon, UserIcon};
use crate::components::NavLink;

#[component]
pub fn FloatingDock() -> impl IntoView {
    view! {
        <aside class="floating-dock rounded-[2rem] shadow-2xl shadow-slate-950/40 flex flex-col">
            <nav class="flex flex-col items-stretch gap-1 p-2">
                <NavLink
                    href="/"
                    class="icon-btn h-12 w-12 mx-auto"
                    title="Feed"
                    aria_label="Feed"
                >
                    <HomeIcon class="h-6 w-6" />
                </NavLink>
                <NavLink
                    href="/posts"
                    class="icon-btn h-12 w-12 mx-auto"
                    title="New post"
                    aria_label="New post"
                >
                    <PlusIcon class="h-6 w-6" />
                </NavLink>
                <NavLink
                    href="/chat"
                    class="icon-btn h-12 w-12 mx-auto"
                    title="Chat"
                    aria_label="Chat"
                >
                    <ChatIcon class="h-6 w-6" />
                </NavLink>
                <NavLink
                    href="/profile"
                    class="icon-btn h-12 w-12 mx-auto"
                    title="Profile"
                    aria_label="Profile"
                >
                    <UserIcon class="h-6 w-6" />
                </NavLink>
            </nav>
        </aside>
    }
}
