// Shared page chrome + the NotFound fallback. AppShell wraps every
// signed-in page in a sticky-header layout; the floating dock is laid
// over the content from each page that wants it.

use leptos::prelude::*;

use crate::components::NavLink;

#[component]
pub fn NotFound() -> impl IntoView {
    view! {
        <section class="min-h-screen flex items-center justify-center p-8 text-center">
            <div class="animate-fade-up">
                <h1 class="logo-handwritten text-7xl text-muted">"404"</h1>
                <p class="mt-3 text-sm text-muted">"Page not found."</p>
                <NavLink
                    href="/"
                    class="unfrost mt-6 inline-flex items-center gap-2 rounded-full bg-accent px-6 py-2.5 text-sm font-semibold text-accent-text shadow-md transition hover:bg-amber-300"
                >
                    "Go home"
                </NavLink>
            </div>
        </section>
    }
}
