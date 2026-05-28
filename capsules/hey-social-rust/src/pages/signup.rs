// SignUp — Rust port of capsules/hey-social/client/src/pages/SignUp.jsx.
//
// In v0.3 the runtime owns the passkey signup flow (it happens in
// System). Hey Social just runs sign-in afterwards. So this page nudges
// the user back to System and offers a link to the sign-in screen.

use leptos::prelude::*;

use crate::components::NavLink;

#[component]
pub fn SignUp() -> impl IntoView {
    view! {
        <section class="relative min-h-[80vh] flex items-center justify-center px-4 py-10">
            <div class="max-w-md w-full">
                <div class="frosted-card p-8 text-center animate-fade-up">
                    <h1 class="logo-handwritten text-4xl text-primary">
                        "Create your passkey in System"
                    </h1>
                    <p class="mt-3 text-sm text-muted">
                        "Hey uses the same passkey across every app on this node. Open System (the home dock), create a passkey, then come back here to sign in."
                    </p>
                    <NavLink
                        href="/"
                        class="unfrost mt-6 inline-flex items-center gap-2 rounded-full bg-accent px-6 py-2.5 text-sm font-semibold text-accent-text shadow-md transition hover:bg-amber-300"
                    >
                        "Back to sign in"
                    </NavLink>
                </div>
            </div>
        </section>
    }
}
