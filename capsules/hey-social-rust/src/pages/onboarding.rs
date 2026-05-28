use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;
use leptos_router::NavigateOptions;
use wasm_bindgen_futures::JsFuture;

#[component]
pub fn Onboarding() -> impl IntoView {
    let navigate = use_navigate();
    let leaving = RwSignal::new(false);

    let go_to_feed = move |_| {
        if leaving.get() {
            return;
        }
        leaving.set(true);
        let navigate = navigate.clone();
        spawn_local(async move {
            // Navigate at warp-out completion (1 s = full keyframe) so
            // the feed's warp-in starts from the same scale + blur the
            // welcome ended at — one continuous tunnel, no seam.
            wait_ms(1000).await;
            navigate("/", NavigateOptions::default());
        });
    };

    view! {
        <section
            class="warp-in relative min-h-[80vh] flex items-center justify-center pl-24 pr-3 py-6 sm:pl-28 sm:pr-6 sm:py-10 overflow-hidden"
            class:warp-transition=move || leaving.get()
        >
            <OnboardingScene />
            <div class="relative z-10 w-full max-w-2xl">
                <div class="frosted-card p-10 sm:p-14 text-center animate-fade-up">
                    <h1 class="logo-handwritten text-6xl sm:text-7xl md:text-8xl text-primary leading-tight">
                        "Welcome to Hey"
                    </h1>
                    <p class="mt-5 text-base text-muted max-w-lg mx-auto leading-7">
                        "You're signed in. Your DID is anchored to your passkey — every Hey app on this node will recognize you automatically. Photos pin to IPFS, posts federate via Carrier, DMs are wrapped in ML-KEM-768 + X25519 hybrid post-quantum crypto."
                    </p>
                    <button
                        type="button"
                        on:click=go_to_feed
                        prop:disabled=move || leaving.get()
                        class="unfrost mt-8 inline-flex items-center gap-2 rounded-full bg-accent px-7 py-3 text-base font-semibold text-accent-text shadow-md transition hover:bg-amber-300 disabled:opacity-60"
                    >
                        {move || if leaving.get() { "Loading…" } else { "Go to feed" }}
                        <svg viewBox="0 0 24 24" class="h-4 w-4" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                            <path d="M5 12h14M13 5l7 7-7 7" />
                        </svg>
                    </button>
                </div>
            </div>
        </section>
    }
}

// Background scene: symbols gently drift in place, never leaving their
// patch of the viewport. The drama (warp) is reserved for the one-shot
// page transition when the user taps "Go to feed" — see the
// .warp-transition class applied to the section root.
//
// Each symbol gets a position + a drift-* keyframe + a staggered
// animation-delay so nothing syncs.
#[component]
fn OnboardingScene() -> impl IntoView {
    view! {
        <div class="pointer-events-none absolute inset-0 overflow-hidden" aria-hidden="true">
            // Slow-drifting gradient blobs anchor the scene.
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

            // Drifting symbols — each pinned to its own corner of the
            // viewport, gently swaying in place.
            <DriftSymbol drift="drift-a" color="sym-warm"    pos="top: 12%; left: 14%; width: 88px; height: 88px;" delay="-1s">
                <circle cx="12" cy="12" r="10" />
            </DriftSymbol>
            <DriftSymbol drift="drift-b" color="sym-sky"     pos="top: 22%; left: 18%; width: 74px; height: 74px;" delay="-5s">
                <path d="M12 3 21 20H3z" />
            </DriftSymbol>
            <DriftSymbol drift="drift-c" color="sym-rose"    pos="bottom: 22%; left: 10%; width: 62px; height: 62px;" delay="-8s">
                <path d="M12 5v14M5 12h14" />
            </DriftSymbol>
            <DriftSymbol drift="drift-d" color="sym-orange"  pos="top: 28%; left: 56%; width: 72px; height: 72px;" delay="-3s">
                <path d="M12 3v4M12 17v4M3 12h4M17 12h4M5.5 5.5l2.8 2.8M15.7 15.7l2.8 2.8M5.5 18.5l2.8-2.8M15.7 8.3l2.8-2.8" />
            </DriftSymbol>
            <DriftSymbol drift="drift-a" color="sym-emerald" pos="top: 58%; right: 16%; width: 84px; height: 84px;" delay="-12s">
                <rect x="3" y="3" width="18" height="18" rx="3" />
            </DriftSymbol>
            <DriftSymbol drift="drift-b" color="sym-violet"  pos="bottom: 32%; right: 30%; width: 108px; height: 108px;" delay="-7s">
                <circle cx="12" cy="12" r="3" />
                <circle cx="12" cy="12" r="7" />
                <circle cx="12" cy="12" r="11" />
            </DriftSymbol>
            <DriftSymbol drift="drift-c" color="sym-indigo"  pos="top: 6%; left: 44%; width: 66px; height: 66px;" delay="-10s">
                <path d="M12 2 22 7v10l-10 5L2 17V7z" />
            </DriftSymbol>
            <DriftSymbol drift="drift-d" color="sym-cyan"    pos="top: 48%; left: 6%; width: 80px; height: 80px;" delay="-15s">
                <rect x="6" y="12" width="12" height="9" rx="2" />
                <path d="M9 12V8a3 3 0 0 1 6 0v4" />
            </DriftSymbol>

            // Filled star — solid fill for variety.
            <svg
                class="absolute drift-a sym-lime"
                style="bottom: 18%; left: 60%; width: 60px; height: 60px; animation-delay: -6s;"
                viewBox="0 0 24 24" fill="currentColor"
            >
                <path d="M12 2 14.6 9.3 22 10l-5.8 4.9L18 22l-6-4-6 4 1.8-7.1L2 10l7.4-.7z" />
            </svg>

            <DriftSymbol drift="drift-c" color="sym-rose"   pos="top: 72%; left: 38%; width: 56px; height: 56px;" delay="-14s">
                <path d="M12 12c-4 0-4-6 0-6s4 6 0 6-4-9 0-9 8 9 0 9-9-12 0-12" />
            </DriftSymbol>
            <DriftSymbol drift="drift-b" color="sym-warm"   pos="top: 34%; right: 8%; width: 58px; height: 58px;" delay="-4s">
                <path d="M12 2 22 12 12 22 2 12z" />
            </DriftSymbol>
        </div>
    }
}

#[component]
fn DriftSymbol(
    #[prop(into)] drift: String,
    #[prop(into)] color: String,
    #[prop(into)] pos: String,
    #[prop(into)] delay: String,
    children: Children,
) -> impl IntoView {
    let class_str = format!("absolute {drift} {color}");
    let style = format!("{pos} animation-delay: {delay};");
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

async fn wait_ms(ms: i32) {
    let win = web_sys::window().unwrap();
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let _ = win
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
    });
    let _ = JsFuture::from(promise).await;
}
