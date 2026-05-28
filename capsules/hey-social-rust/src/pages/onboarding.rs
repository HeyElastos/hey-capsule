// Onboarding — first-run welcome card. The React reference's onboarding
// is a multi-step wizard (capsules/hey-social/client/src/pages/Onboarding.jsx
// is 367 lines); the Rust port stays minimal — a single welcome screen
// that sends the user to /home. Polish parity is a follow-up.

use leptos::prelude::*;

use crate::components::NavLink;

#[component]
pub fn Onboarding() -> impl IntoView {
    view! {
        <section class="relative min-h-[80vh] flex items-center justify-center px-4 py-10">
            <div class="max-w-md w-full">
                <div class="frosted-card p-8 text-center animate-fade-up">
                    <h1 class="logo-handwritten text-5xl text-primary">
                        "Welcome to Hey"
                    </h1>
                    <p class="mt-3 text-sm text-muted">
                        "You're signed in. Your DID is anchored to your passkey — every Hey app on this node will recognize you automatically."
                    </p>
                    <NavLink
                        href="/"
                        class="unfrost mt-6 inline-flex items-center gap-2 rounded-full bg-accent px-6 py-2.5 text-sm font-semibold text-accent-text shadow-md transition hover:bg-amber-300"
                    >
                        "Go to feed"
                    </NavLink>
                </div>
            </div>
        </section>
    }
}
