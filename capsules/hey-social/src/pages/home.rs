// Home — 1:1 port of capsules/hey-social/client/src/pages/Home.jsx.
//
// Gates on session: signed-out users see the Landing page directly (same
// component, no redirect — matches React behavior). Signed-in users get
// the photo-feed: skeleton during load, frosted empty-state when no
// posts, or a fade-up stack of PostCards.

use leptos::prelude::*;
use leptos::task::spawn_local;
use crate::api::posts::{get_posts, Post};
use crate::components::icons::CameraIcon;
use crate::components::{FloatingDock, NavLink, PostCard, TopHeader};
use crate::pages::landing::Landing;
use crate::session;

#[component]
pub fn Home() -> impl IntoView {
    let user = session::current();
    if user.is_none() {
        return view! { <Landing /> }.into_any();
    }

    // Boot splash → feed tunnel. On first paint the #hey-boot splash (the
    // "Welcome to Hey" screen injected in index.html) is still covering the
    // viewport. Let it read for a beat, then warp it out as this feed flies
    // in — reusing the .warp-in already wrapping the feed below, so the
    // splash and feed share one continuous warp. Idempotent: a later
    // home-navigation finds the splash already dismissed and no-ops.
    Effect::new(|_| {
        spawn_local(async {
            crate::runtime::boot_log("home: feed ready — warping splash into feed");
            crate::runtime::sleep_ms(850).await;
            crate::runtime::warp_boot_into_feed();
        });
    });

    let posts: RwSignal<Vec<Post>> = RwSignal::new(Vec::new());
    let loading = RwSignal::new(true);
    let error = RwSignal::new(String::new());

    Effect::new(move |_| {
        loading.set(true);
        error.set(String::new());
        spawn_local(async move {
            match get_posts(50).await {
                Ok(p) => {
                    posts.set(p);
                    loading.set(false);
                }
                Err(_) => {
                    error.set("Unable to load feed.".into());
                    loading.set(false);
                }
            }
        });
    });

    let photo_posts = Memo::new(move |_| {
        posts
            .read()
            .iter()
            .filter(|p| {
                p.images
                    .first()
                    .map(|m| m.media_type != "video")
                    .unwrap_or(true)
            })
            .cloned()
            .collect::<Vec<_>>()
    });

    view! {
        // Chrome (TopHeader + FloatingDock) gets its OWN opacity-only
        // fade-in. We can't put it inside .warp-in: the floating dock
        // uses position: fixed, and a transformed ancestor re-anchors
        // fixed children to its bounding box — during the warp the dock
        // would shrink and dance with the feed. Two siblings, two
        // animations: chrome fades, feed warps.
        <>
        <div class="warp-chrome-in">
            <TopHeader />
            <FloatingDock />
        </div>
        <div class="warp-in">
            <div class="mx-auto max-w-2xl space-y-6 pl-24 pr-3 py-6 sm:pl-28 sm:pr-6 sm:py-10">
                {move || if loading.get() {
                    view! { <FeedSkeleton /> }.into_any()
                } else if !error.get().is_empty() {
                    view! {
                        <div class="frosted-card animate-fade-in p-4 text-sm text-red-400">
                            {error.get()}
                        </div>
                    }.into_any()
                } else if photo_posts.read().is_empty() {
                    view! { <EmptyState /> }.into_any()
                } else {
                    view! {
                        <For
                            each=move || photo_posts.get()
                            key=|p| p.id.clone()
                            children=move |post: Post| view! {
                                <div class="animate-fade-up">
                                    <PostCard post=post />
                                </div>
                            }
                        />
                    }.into_any()
                }}
            </div>
        </div>
        </>
    }.into_any()
}

#[component]
fn FeedSkeleton() -> impl IntoView {
    view! {
        <div class="space-y-6">
            {(0..2).map(|i| view! {
                <div
                    class="frosted-card overflow-hidden p-0 animate-fade-in"
                    style=format!("animation-delay: {}ms", i * 100)
                >
                    <div class="flex items-center gap-3 p-4">
                        <div class="h-10 w-10 rounded-full image-skeleton" />
                        <div class="space-y-2">
                            <div class="h-3 w-32 rounded image-skeleton" />
                            <div class="h-2 w-16 rounded image-skeleton" />
                        </div>
                    </div>
                    <div class="aspect-square image-skeleton" />
                    <div class="space-y-2 p-4">
                        <div class="h-3 w-3/4 rounded image-skeleton" />
                        <div class="h-3 w-1/2 rounded image-skeleton" />
                    </div>
                </div>
            }).collect::<Vec<_>>()}
        </div>
    }
}

#[component]
fn EmptyState() -> impl IntoView {
    view! {
        <div class="empty-state-wrap">
        <div class="frosted-card relative overflow-hidden animate-fade-up p-10 text-center w-full max-w-md">
            <div
                class="relative mx-auto flex h-16 w-16 items-center justify-center rounded-2xl border border-white/20 bg-white/10 shadow-lg shadow-slate-900/20 backdrop-blur-xl dark:bg-white/[0.06]"
                style="-webkit-backdrop-filter: blur(20px)"
            >
                <CameraIcon class="h-7 w-7 text-accent" />
            </div>

            <h2 class="mt-5 logo-handwritten text-4xl text-primary sm:text-5xl">
                "Your feed is empty"
            </h2>
            <p class="mx-auto mt-3 max-w-sm text-sm leading-6 text-muted">
                "Be the first to drop a photo. A view from your window, your morning coffee — anything counts. Your followers' feeds start with you."
            </p>

            <div class="relative mt-6 inline-block">
                <span
                    aria-hidden="true"
                    class="caret-cue absolute -top-3 -right-4 sm:-right-6 rounded-full border-2 border-slate-900 bg-accent px-2 py-0.5 text-[10px] font-bold uppercase tracking-wider text-accent-text shadow-[2px_2px_0_rgba(15,23,42,1)]"
                >
                    "Start here"
                </span>
                <NavLink
                    href="/posts"
                    style="background-color: rgb(34 197 94)"
                    class="group inline-flex items-center gap-2 rounded-full border-2 border-green-600 px-6 py-2.5 text-sm font-semibold text-white shadow-md shadow-green-900/30 transition hover:!bg-green-600"
                >
                    "Share your first photo"
                    <svg
                        viewBox="0 0 24 24"
                        class="h-4 w-4 fill-none stroke-current stroke-[2] transition-transform duration-200 group-hover:translate-x-1"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    >
                        <path d="M5 12h14M13 5l7 7-7 7" />
                    </svg>
                </NavLink>
            </div>
        </div>
        </div>
    }
}
