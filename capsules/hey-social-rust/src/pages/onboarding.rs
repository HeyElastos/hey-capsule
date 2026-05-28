// Onboarding — first-run welcome card. The React reference's onboarding
// is a multi-step wizard (capsules/hey-social/client/src/pages/Onboarding.jsx
// is 367 lines); the Rust port stays minimal — a single welcome screen
// that sends the user to /home. Polish parity is a follow-up.

use leptos::prelude::*;

use crate::components::NavLink;

#[component]
pub fn Onboarding() -> impl IntoView {
    view! {
        <section class="relative min-h-[80vh] flex items-center justify-center pl-24 pr-3 py-6 sm:pl-28 sm:pr-6 sm:py-10 overflow-hidden">
            <OnboardingScene />
            <div class="relative z-10 w-full max-w-2xl">
                <div class="frosted-card p-10 sm:p-14 text-center animate-fade-up">
                    <h1 class="logo-handwritten text-5xl sm:text-6xl text-primary">
                        "Welcome to Hey"
                    </h1>
                    <p class="mt-5 text-base text-muted max-w-lg mx-auto leading-7">
                        "You're signed in. Your DID is anchored to your passkey — every Hey app on this node will recognize you automatically. Photos pin to IPFS, posts federate via Carrier, DMs are wrapped in ML-KEM-768 + X25519 hybrid post-quantum crypto."
                    </p>
                    <NavLink
                        href="/"
                        class="unfrost mt-8 inline-flex items-center gap-2 rounded-full bg-accent px-7 py-3 text-base font-semibold text-accent-text shadow-md transition hover:bg-amber-300"
                    >
                        "Go to feed"
                    </NavLink>
                </div>
            </div>
        </section>
    }
}

// Background scene: floating + glowing abstract symbols. Uses the
// existing animation classes from styles.css — float-shape with
// shape-{a,b,c,d,gentle}, glow (gradient-pulse), and square-tick
// (slow 48s rotation). No JS, all CSS keyframes.
#[component]
fn OnboardingScene() -> impl IntoView {
    view! {
        <div class="pointer-events-none absolute inset-0 overflow-hidden" aria-hidden="true">
            // Three soft gradient blobs — gold / blue / pink — drifting at different rhythms.
            <div
                class="float-shape glow"
                style="top: 8%; left: 6%; width: 380px; height: 380px;
                       background: radial-gradient(circle closest-side at center,
                         rgba(212,184,75,0.65) 0%, rgba(212,184,75,0.22) 40%, transparent 75%);
                       filter: blur(75px);"
            />
            <div
                class="float-shape glow"
                style="bottom: 6%; right: 4%; width: 480px; height: 480px;
                       background: radial-gradient(circle closest-side at center,
                         rgba(96,165,250,0.55) 0%, rgba(96,165,250,0.18) 40%, transparent 75%);
                       filter: blur(90px); animation-delay: 1.4s;"
            />
            <div
                class="float-shape glow"
                style="top: 42%; right: 22%; width: 300px; height: 300px;
                       background: radial-gradient(circle closest-side at center,
                         rgba(244,114,182,0.50) 0%, rgba(244,114,182,0.18) 40%, transparent 75%);
                       filter: blur(70px); animation-delay: 2.8s;"
            />
            <div
                class="float-shape glow"
                style="top: 64%; left: 28%; width: 240px; height: 240px;
                       background: radial-gradient(circle closest-side at center,
                         rgba(52,211,153,0.45) 0%, rgba(52,211,153,0.15) 40%, transparent 75%);
                       filter: blur(60px); animation-delay: 0.6s;"
            />

            // Outline circle (slow float, top-right).
            <svg
                class="float-shape shape-a text-amber-700/45 dark:text-accent/65"
                style="top: 12%; right: 14%; width: 92px; height: 92px;"
                viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1"
            >
                <circle cx="12" cy="12" r="10" />
            </svg>

            // Triangle.
            <svg
                class="float-shape shape-b text-sky-700/50 dark:text-sky-300/75"
                style="top: 22%; left: 16%; width: 78px; height: 78px;"
                viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.25" stroke-linejoin="round"
            >
                <path d="M12 3 21 20H3z" />
            </svg>

            // Plus.
            <svg
                class="float-shape shape-c text-pink-600/55 dark:text-pink-300/75"
                style="bottom: 22%; left: 10%; width: 62px; height: 62px;"
                viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"
            >
                <path d="M12 5v14M5 12h14" />
            </svg>

            // Sparkle / sun.
            <svg
                class="float-shape shape-d text-amber-600/75 dark:text-amber-200/85"
                style="top: 26%; left: 56%; width: 72px; height: 72px;"
                viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.25" stroke-linecap="round"
            >
                <path d="M12 3v4M12 17v4M3 12h4M17 12h4M5.5 5.5l2.8 2.8M15.7 15.7l2.8 2.8M5.5 18.5l2.8-2.8M15.7 8.3l2.8-2.8" />
            </svg>

            // Slow-rotating square.
            <div
                class="float-shape shape-c"
                style="top: 58%; right: 6%; width: 70px; height: 70px; animation-delay: 0.7s;"
            >
                <svg class="square-tick text-emerald-700/45 dark:text-emerald-300/70" style="width: 100%; height: 100%;" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.25">
                    <rect x="3" y="3" width="18" height="18" rx="3" />
                </svg>
            </div>

            // Concentric circles (mid-left).
            <svg
                class="float-shape shape-gentle text-fuchsia-500/50 dark:text-fuchsia-300/70"
                style="bottom: 32%; right: 30%; width: 120px; height: 120px;"
                viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1"
            >
                <circle cx="12" cy="12" r="3" />
                <circle cx="12" cy="12" r="7" />
                <circle cx="12" cy="12" r="11" />
            </svg>

            // Hexagon (top center).
            <svg
                class="float-shape shape-a text-indigo-500/45 dark:text-indigo-300/70"
                style="top: 6%; left: 42%; width: 64px; height: 64px; animation-delay: 1.8s;"
                viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.25" stroke-linejoin="round"
            >
                <path d="M12 2 22 7v10l-10 5L2 17V7z" />
            </svg>

            // Star (lower right area).
            <svg
                class="float-shape shape-d text-yellow-500/65 dark:text-yellow-300/85"
                style="bottom: 18%; left: 60%; width: 60px; height: 60px; animation-delay: 2.2s;"
                viewBox="0 0 24 24" fill="currentColor"
            >
                <path d="M12 2 14.6 9.3 22 10l-5.8 4.9L18 22l-6-4-6 4 1.8-7.1L2 10l7.4-.7z" />
            </svg>

            // Diamond/lock combo — riffs on the encrypted nature of the app.
            <svg
                class="float-shape shape-b text-cyan-500/55 dark:text-cyan-300/75"
                style="top: 48%; left: 6%; width: 72px; height: 72px; animation-delay: 3.1s;"
                viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" stroke-linejoin="round"
            >
                <rect x="6" y="12" width="12" height="9" rx="2" />
                <path d="M9 12V8a3 3 0 0 1 6 0v4" />
            </svg>
        </div>
    }
}
