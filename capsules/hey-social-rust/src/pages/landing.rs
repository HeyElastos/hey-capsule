// Landing — 1:1 port of capsules/hey-social/client/src/pages/Landing.jsx.
//
// Same FloatingScene (gradient blobs + parallax SVG primitives), same
// HeyMark (Dancing-Script wordmark with pencil-stroke draw + underline),
// same passkey + recovery-key dual path. Uses the React capsule's
// compiled stylesheet (styles.css, shipped alongside the WASM) verbatim
// so class names like .frosted-card / .float-shape / .hey-pencil /
// .hey-wordmark / .animate-fade-up resolve identically.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;
use leptos_router::NavigateOptions;

use crate::passkey::{passkey_supported, sign_in_via_runtime};
use crate::session;

#[component]
pub fn Landing() -> impl IntoView {
    let navigate = use_navigate();

    // If already signed in, skip the landing entirely.
    Effect::new({
        let navigate = navigate.clone();
        move |_| {
            if session::current().is_some() {
                navigate("/home", NavigateOptions::default());
            }
        }
    });

    let busy = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let can_use_passkey = passkey_supported();

    let handle_passkey = {
        let navigate = navigate.clone();
        move |_| {
            if busy.get() {
                return;
            }
            error.set(String::new());
            busy.set(true);
            let navigate = navigate.clone();
            spawn_local(async move {
                match sign_in_via_runtime(None).await {
                    Ok(_session) => {
                        busy.set(false);
                        // Matches the React Landing handler: route to /welcome
                        // so first-time users see Onboarding before the feed.
                        navigate("/welcome", NavigateOptions::default());
                    }
                    Err(msg) => {
                        busy.set(false);
                        if msg.contains("NotAllowedError")
                            || msg.contains("AbortError")
                            || msg.to_lowercase().contains("cancel")
                        {
                            error.set("Passkey prompt closed. Tap to try again.".into());
                        } else {
                            error.set(msg);
                        }
                    }
                }
            });
        }
    };

    view! {
        <div class="relative -mt-10 flex min-h-[80vh] flex-col items-center justify-center px-4 py-10">
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
                    "Photo, video, and chat — peer-to-peer over Elastos. Sign in with the same passkey you used to set up this device. No password, no recovery key."
                </p>

                <div
                    class="relative mx-auto mt-12 max-w-sm animate-fade-up"
                    style="animation-delay: 1.3s"
                >
                    {move || if can_use_passkey {
                        view! {
                            <button
                                type="button"
                                on:click=handle_passkey.clone()
                                prop:disabled=move || busy.get()
                                class="unfrost group inline-flex w-full items-center justify-center gap-3 rounded-full bg-accent px-8 py-4 text-base font-semibold text-accent-text shadow-xl shadow-slate-900/25 transition hover:bg-amber-300 disabled:cursor-not-allowed disabled:opacity-60"
                            >
                                <svg viewBox="0 0 24 24" class="h-5 w-5 fill-current">
                                    <path d="M12 2a5 5 0 0 0-5 5v3H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8a2 2 0 0 0-2-2h-1V7a5 5 0 0 0-5-5Zm-3 8V7a3 3 0 0 1 6 0v3H9Z" />
                                </svg>
                                {move || if busy.get() { "Waiting for passkey…" } else { "Sign in with passkey" }}
                            </button>
                        }.into_any()
                    } else {
                        view! {
                            <div class="frosted-card p-5 text-sm text-muted">
                                "Your browser doesn't support passkeys. Hey needs a passkey-capable browser (modern Chrome / Edge / Safari / Firefox)."
                            </div>
                        }.into_any()
                    }}

                    {move || {
                        let msg = error.get();
                        if msg.is_empty() { view! { <></> }.into_any() }
                        else {
                            view! {
                                <p class="mt-4 animate-fade-in text-sm text-red-400">{msg}</p>
                            }.into_any()
                        }
                    }}

                    <p
                        class="mt-8 text-xs text-muted animate-fade-in"
                        style="animation-delay: 1.6s"
                    >
                        "One tap. Same passkey as System. Nothing to remember."
                    </p>
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
