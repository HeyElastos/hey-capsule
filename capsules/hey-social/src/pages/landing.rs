// Landing / sign-in page — runtime-only (wallet capsule model).
//
// Keeps the original look (FloatingScene gradient blobs + the HeyMark
// Dancing-Script wordmark) but there is NO in-capsule auth: identity comes
// ONLY from the Elastos runtime — the identity provider (`identity/whoami`,
// a provider-backed did:key with NO local seed) or an inherited runtime
// session (`/api/session`, wallet SSO from Home's launch token). Without the
// runtime the app does not open. On success we navigate to the feed.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;
use leptos_router::NavigateOptions;

use crate::session;

#[component]
pub fn Landing() -> impl IntoView {
    let navigate = use_navigate();
    let checking = RwSignal::new(true);
    let offline = RwSignal::new(false);

    // Ask the runtime who we are: an already-persisted runtime session, then the
    // identity provider (no local seed), then an inherited runtime session. On
    // success → feed; otherwise show the runtime-required state. There is NO
    // passkey / local-seed fallback — auth lives in the runtime, not here.
    let probe = {
        let navigate = navigate.clone();
        move || {
            let navigate = navigate.clone();
            checking.set(true);
            offline.set(false);
            spawn_local(async move {
                if session::current().is_some() {
                    navigate("/home", NavigateOptions::default());
                    return;
                }
                crate::runtime::boot_log("landing: no-tap adoption (identity/whoami)");
                let mut ok = crate::api::dms::adopt_provider_identity().await.is_some();
                if !ok {
                    crate::runtime::boot_log("landing: no provider identity; trying legacy inherit");
                    if let Some(inherited) = crate::runtime::inherit_session().await {
                        session::set(&inherited);
                        ok = true;
                    }
                }
                if ok {
                    crate::runtime::boot_log("landing: signed in — navigating to feed");
                    navigate("/home", NavigateOptions::default());
                } else {
                    crate::runtime::boot_log("landing: runtime unreachable — gated");
                    crate::runtime::hide_boot_splash();
                    checking.set(false);
                    offline.set(true);
                }
            });
        }
    };

    // Probe once on mount.
    {
        let probe = probe.clone();
        Effect::new(move |_| {
            probe();
        });
    }

    let retry = {
        let probe = probe.clone();
        move |_| probe()
    };

    view! {
        <div class="relative flex min-h-screen flex-col items-center justify-center px-4 py-10">
            <FloatingScene />

            <div class="relative z-10 mx-auto max-w-2xl text-center">
                <p
                    class="mb-6 text-xs uppercase tracking-[0.4em] text-muted animate-fade-in"
                    style="animation-delay: 0.6s"
                >
                    "Your own social media on Elastos"
                </p>

                <HeyMark />

                <p
                    class="mx-auto mt-4 max-w-lg text-base leading-7 text-muted animate-fade-up"
                    style="animation-delay: 1.0s"
                >
                    "Photo, video, and chat — peer-to-peer over Elastos. Your identity comes from your Elastos runtime; there's nothing to sign in here."
                </p>

                <div
                    class="relative mx-auto mt-12 max-w-sm animate-fade-up"
                    style="animation-delay: 1.3s"
                >
                    {move || if checking.get() {
                        view! {
                            <p class="text-sm text-muted">"Connecting to your Elastos runtime…"</p>
                        }.into_any()
                    } else if offline.get() {
                        let retry = retry.clone();
                        view! {
                            <div class="frosted-card p-5 text-sm text-muted">
                                <p>
                                    "Hey gets your identity from your Elastos runtime. It isn't reachable right now — open Hey from your runtime's Home, then retry."
                                </p>
                                <button
                                    type="button"
                                    on:click=retry
                                    class="mt-4 inline-flex w-full items-center justify-center rounded-full bg-white/12 hover:bg-white/22 border border-white/25 backdrop-blur-xl px-8 py-3 text-base font-semibold text-primary transition hover:-translate-y-0.5"
                                >
                                    "Retry"
                                </button>
                            </div>
                        }.into_any()
                    } else {
                        ().into_any()
                    }}
                </div>
            </div>
        </div>
    }
}

#[component]
fn FloatingScene() -> impl IntoView {
    view! {
        <div class="pointer-events-none absolute inset-0 overflow-hidden" aria-hidden="true">
            // Three gradient glow blobs — closest-side keeps the colored area inside.
            <div
                class="float-shape glow"
                style="top: 6%; left: 8%; width: 420px; height: 420px; background: radial-gradient(circle closest-side at center, rgba(212,184,75,0.75) 0%, rgba(212,184,75,0.30) 40%, transparent 75%); filter: blur(80px);"
            />
            <div
                class="float-shape glow"
                style="bottom: 8%; right: 8%; width: 520px; height: 520px; background: radial-gradient(circle closest-side at center, rgba(96,165,250,0.60) 0%, rgba(96,165,250,0.22) 40%, transparent 75%); filter: blur(90px); animation-delay: 1.5s;"
            />
            <div
                class="float-shape glow"
                style="top: 38%; right: 26%; width: 320px; height: 320px; background: radial-gradient(circle closest-side at center, rgba(244,114,182,0.50) 0%, rgba(244,114,182,0.18) 40%, transparent 75%); filter: blur(70px); animation-delay: 3s;"
            />

            // Outline circle
            <svg
                class="float-shape shape-a text-amber-700/40 dark:text-accent/60"
                style="top: 14%; right: 16%; width: 80px; height: 80px;"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1"
            >
                <circle cx="12" cy="12" r="10" />
            </svg>

            // Triangle
            <svg
                class="float-shape shape-b text-sky-700/45 dark:text-sky-300/70"
                style="top: 22%; left: 18%; width: 70px; height: 70px;"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.25"
                stroke-linejoin="round"
            >
                <path d="M12 3 21 20H3z" />
            </svg>

            // Plus
            <svg
                class="float-shape shape-c text-pink-600/50 dark:text-pink-300/70"
                style="bottom: 26%; left: 12%; width: 56px; height: 56px;"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
            >
                <path d="M12 5v14M5 12h14" />
            </svg>

            // Sparkle / sun above the "y"
            <svg
                class="float-shape shape-d text-amber-600/70 dark:text-amber-200/80"
                style="top: 20%; left: 58%; width: 64px; height: 64px;"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.25"
                stroke-linecap="round"
            >
                <path d="M12 3v4M12 17v4M3 12h4M17 12h4M5.5 5.5l2.8 2.8M15.7 15.7l2.8 2.8M5.5 18.5l2.8-2.8M15.7 8.3l2.8-2.8" />
            </svg>

            // Square outline (slow rotation via .square-tick)
            <div
                class="float-shape shape-c"
                style="top: 62%; right: 8%; width: 60px; height: 60px; animation-delay: 0.7s;"
            >
                <svg
                    class="square-tick text-emerald-700/40 dark:text-emerald-300/60"
                    style="width: 100%; height: 100%;"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.25"
                >
                    <rect x="3" y="3" width="18" height="18" rx="3" />
                </svg>
            </div>
        </div>
    }
}

#[component]
fn HeyMark() -> impl IntoView {
    view! {
        <div class="relative inline-block pb-8">
            <svg
                class="hey-underline absolute left-1/2 -translate-x-1/2 -z-10"
                style="bottom: 22%; width: 88%; opacity: 0.85;"
                viewBox="0 0 240 30"
                fill="none"
                stroke="currentColor"
                stroke-width="5"
                stroke-linecap="round"
            >
                <path d="M8 18 Q60 4, 120 14 T232 12" class="text-accent" />
            </svg>

            <svg
                viewBox="0 0 480 280"
                class="hey-wordmark relative block mx-auto w-[280px] sm:w-[420px]"
                aria-label="Hey"
            >
                <defs>
                    <mask id="hey-mask-0">
                        <text
                            x="110"
                            y="200"
                            class="hey-pencil"
                            style="font-family: 'Dancing Script', cursive; font-weight: 600; font-size: 200px; animation-delay: 0s;"
                        >"H"</text>
                    </mask>
                    <mask id="hey-mask-1">
                        <text
                            x="230"
                            y="200"
                            class="hey-pencil"
                            style="font-family: 'Dancing Script', cursive; font-weight: 600; font-size: 200px; animation-delay: 0.9s;"
                        >"e"</text>
                    </mask>
                    <mask id="hey-mask-2">
                        <text
                            x="320"
                            y="200"
                            class="hey-pencil"
                            style="font-family: 'Dancing Script', cursive; font-weight: 600; font-size: 200px; animation-delay: 1.8s;"
                        >"y"</text>
                    </mask>
                </defs>

                <text x="110" y="200" class="hey-fill" mask="url(#hey-mask-0)" style="font-family: 'Dancing Script', cursive; font-weight: 600; font-size: 200px;">"H"</text>
                <text x="230" y="200" class="hey-fill" mask="url(#hey-mask-1)" style="font-family: 'Dancing Script', cursive; font-weight: 600; font-size: 200px;">"e"</text>
                <text x="320" y="200" class="hey-fill" mask="url(#hey-mask-2)" style="font-family: 'Dancing Script', cursive; font-weight: 600; font-size: 200px;">"y"</text>
            </svg>
        </div>
    }
}
