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

// Background scene: abstract symbols continuously fly across the screen.
// Each symbol picks one of the .fly-a / .fly-b / .fly-c keyframes from
// welcome-animations.css and rides a staggered animation-delay so the
// flow feels endless (a new symbol enters whenever an older one exits).
// Color is set via the .sym-* classes which also apply a matching glow
// drop-shadow so symbols pop against the dark radial-gradient body.
#[component]
fn OnboardingScene() -> impl IntoView {
    view! {
        <div class="pointer-events-none absolute inset-0 overflow-hidden" aria-hidden="true">
            // Slow-drifting gradient blobs — anchor the scene so the
            // flying symbols don't feel adrift on flat black.
            <div
                class="absolute glow-drift"
                style="top: 8%; left: 6%; width: 380px; height: 380px;
                       background: radial-gradient(circle closest-side at center,
                         rgba(212,184,75,0.65) 0%, rgba(212,184,75,0.22) 40%, transparent 75%);
                       filter: blur(75px);"
            />
            <div
                class="absolute glow-drift"
                style="bottom: 6%; right: 4%; width: 480px; height: 480px;
                       background: radial-gradient(circle closest-side at center,
                         rgba(96,165,250,0.55) 0%, rgba(96,165,250,0.18) 40%, transparent 75%);
                       filter: blur(90px); animation-delay: -3s;"
            />
            <div
                class="absolute glow-drift"
                style="top: 42%; right: 22%; width: 300px; height: 300px;
                       background: radial-gradient(circle closest-side at center,
                         rgba(244,114,182,0.50) 0%, rgba(244,114,182,0.18) 40%, transparent 75%);
                       filter: blur(70px); animation-delay: -6s;"
            />
            <div
                class="absolute glow-drift"
                style="top: 64%; left: 28%; width: 240px; height: 240px;
                       background: radial-gradient(circle closest-side at center,
                         rgba(52,211,153,0.45) 0%, rgba(52,211,153,0.15) 40%, transparent 75%);
                       filter: blur(60px); animation-delay: -9s;"
            />

            // Flying symbols. The negative animation-delay starts each
            // one mid-keyframe so the scene is fully populated on first
            // paint (no awkward "wait for symbols to enter").
            <FlyingSymbol class_str="absolute fly-a sym-warm" base="top: 12%; left: 14%; width: 92px; height: 92px;" delay="-2s">
                <circle cx="12" cy="12" r="10" />
            </FlyingSymbol>

            <FlyingSymbol class_str="absolute fly-b sym-sky" base="top: 22%; left: 16%; width: 78px; height: 78px;" delay="-7s">
                <path d="M12 3 21 20H3z" />
            </FlyingSymbol>

            <FlyingSymbol class_str="absolute fly-c sym-rose" base="bottom: 22%; left: 10%; width: 64px; height: 64px;" delay="-4s">
                <path d="M12 5v14M5 12h14" />
            </FlyingSymbol>

            <FlyingSymbol class_str="absolute fly-a sym-orange" base="top: 26%; left: 56%; width: 76px; height: 76px;" delay="-13s">
                <path d="M12 3v4M12 17v4M3 12h4M17 12h4M5.5 5.5l2.8 2.8M15.7 15.7l2.8 2.8M5.5 18.5l2.8-2.8M15.7 8.3l2.8-2.8" />
            </FlyingSymbol>

            <FlyingSymbol class_str="absolute fly-b sym-emerald" base="top: 58%; right: 16%; width: 84px; height: 84px;" delay="-18s">
                <rect x="3" y="3" width="18" height="18" rx="3" />
            </FlyingSymbol>

            <FlyingSymbol class_str="absolute fly-c sym-violet" base="bottom: 32%; right: 30%; width: 110px; height: 110px;" delay="-10s">
                <circle cx="12" cy="12" r="3" />
                <circle cx="12" cy="12" r="7" />
                <circle cx="12" cy="12" r="11" />
            </FlyingSymbol>

            <FlyingSymbol class_str="absolute fly-a sym-indigo" base="top: 6%; left: 42%; width: 68px; height: 68px;" delay="-15s">
                <path d="M12 2 22 7v10l-10 5L2 17V7z" />
            </FlyingSymbol>

            // Filled star (slight variation on viewBox stroke vs fill).
            <svg
                class="absolute fly-b sym-lime"
                style="bottom: 18%; left: 60%; width: 64px; height: 64px; animation-delay: -20s;"
                viewBox="0 0 24 24" fill="currentColor"
            >
                <path d="M12 2 14.6 9.3 22 10l-5.8 4.9L18 22l-6-4-6 4 1.8-7.1L2 10l7.4-.7z" />
            </svg>

            <FlyingSymbol class_str="absolute fly-c sym-cyan" base="top: 48%; left: 6%; width: 80px; height: 80px;" delay="-12s">
                <rect x="6" y="12" width="12" height="9" rx="2" />
                <path d="M9 12V8a3 3 0 0 1 6 0v4" />
            </FlyingSymbol>

            // Two more for density — a small spiral and a diamond.
            <FlyingSymbol class_str="absolute fly-a sym-rose" base="top: 72%; left: 38%; width: 58px; height: 58px;" delay="-23s">
                <path d="M12 12c-4 0-4-6 0-6s4 6 0 6-4-9 0-9 8 9 0 9-9-12 0-12" />
            </FlyingSymbol>

            <FlyingSymbol class_str="absolute fly-b sym-warm" base="top: 34%; right: 8%; width: 60px; height: 60px;" delay="-5s">
                <path d="M12 2 22 12 12 22 2 12z" />
            </FlyingSymbol>
        </div>
    }
}

#[component]
fn FlyingSymbol(
    #[prop(into)] class_str: String,
    #[prop(into)] base: String,
    #[prop(into)] delay: String,
    children: Children,
) -> impl IntoView {
    let style = format!("{base} animation-delay: {delay};");
    view! {
        <svg
            class=class_str
            style=style
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="1.25"
            stroke-linecap="round"
            stroke-linejoin="round"
        >
            {children()}
        </svg>
    }
}
