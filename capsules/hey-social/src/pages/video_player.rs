// VideoPlayer — Rust port of capsules/hey-social/client/src/pages/VideoPlayer.jsx.
//
// Two-tier playback:
//   1. Plain mp4/webm from IPFS → native <video> element. Works for the
//      vast majority of posts uploaded via the Posts page (whatever the
//      camera produces).
//   2. DASH/CENC / unusual codecs / DRM → delegate to the Elacity Player
//      capsule over the provider bus. It serves an iframe URL we mount
//      sandboxed.
//
// The Rust port tries the native path first and only falls through to
// elacity if the video element fires an error. Elacity provider call:
// runtime::provider_call("elacity", "embed", { src }) → { url }.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_params_map;
use serde_json::json;

use crate::api::posts::{get_post, Post};
use crate::runtime::{ipfs, provider_call};

#[component]
pub fn VideoPlayer() -> impl IntoView {
    let params = use_params_map();
    let post: RwSignal<Option<Post>> = RwSignal::new(None);
    let elacity_url: RwSignal<Option<String>> = RwSignal::new(None);
    let use_native = RwSignal::new(true);

    Effect::new(move |_| {
        let id = params.read().get("id").map(|s| s.to_string()).unwrap_or_default();
        if id.is_empty() {
            return;
        }
        spawn_local(async move {
            if let Ok(Some(p)) = get_post(&id).await {
                post.set(Some(p));
            }
        });
    });

    let try_elacity = move |_| {
        use_native.set(false);
        let post_val = post.get();
        spawn_local(async move {
            let Some(p) = post_val else { return };
            let Some(media) = p.images.first() else { return };
            let src = format!("elastos://{}", media.cid);
            match provider_call("elacity", "embed", json!({ "src": src, "autoPlay": true })).await {
                Ok(resp) => {
                    if let Some(url) = resp.get("url").and_then(|u| u.as_str()) {
                        elacity_url.set(Some(url.to_string()));
                    }
                }
                Err(_) => {
                    // No Elacity capsule installed; nothing to fall back to.
                }
            }
        });
    };

    view! {
        <>
            <div class="mx-auto max-w-2xl space-y-4 pl-24 pr-3 py-6 sm:pl-28 sm:pr-6 sm:py-10">
                {move || match post.get() {
                    None => view! {
                        <p class="text-sm text-muted">"Loading…"</p>
                    }.into_any(),
                    Some(p) => {
                        let media = p.images.first().cloned();
                        let title = p.caption.clone();
                        view! {
                            <div class="frosted-card overflow-hidden p-0 bg-black">
                                {match media {
                                    Some(m) if use_native.get() => view! {
                                        <video
                                            controls
                                            class="block w-full"
                                            src=ipfs::gateway_url(&m.cid, None)
                                            on:error=try_elacity.clone()
                                        />
                                    }.into_any(),
                                    Some(_) => match elacity_url.get() {
                                        Some(url) => view! {
                                            <iframe
                                                class="block w-full aspect-video"
                                                src=url
                                                allow="autoplay; encrypted-media; fullscreen"
                                                sandbox="allow-scripts allow-same-origin"
                                            />
                                        }.into_any(),
                                        None => view! {
                                            <p class="p-6 text-sm text-slate-300">
                                                "Couldn't play this video natively. Install the Elacity Player capsule to enable DASH/CENC playback."
                                            </p>
                                        }.into_any(),
                                    },
                                    None => view! {
                                        <p class="p-6 text-sm text-slate-300">"This post has no media."</p>
                                    }.into_any(),
                                }}
                            </div>
                            <h2 class="text-base font-medium text-primary">{title}</h2>
                        }.into_any()
                    }
                }}
            </div>
        </>
    }
}
