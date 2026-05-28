// Clips — short-form video feed. Same data source as Home, filtered to
// posts whose first media is a video. Rust port of capsules/hey-social/
// client/src/pages/Clips.jsx.

use leptos::prelude::*;
use leptos::task::spawn_local;
use crate::api::posts::{get_posts, Post};
use crate::components::icons::VideoIcon;
use crate::components::{FloatingDock, NavLink, PostCard, TopHeader};

#[component]
pub fn Clips() -> impl IntoView {
    let posts: RwSignal<Vec<Post>> = RwSignal::new(Vec::new());
    let loading = RwSignal::new(true);

    Effect::new(move |_| {
        loading.set(true);
        spawn_local(async move {
            let p = get_posts(50).await.unwrap_or_default();
            posts.set(p);
            loading.set(false);
        });
    });

    let video_posts = Memo::new(move |_| {
        posts
            .read()
            .iter()
            .filter(|p| p.images.first().map(|m| m.media_type == "video").unwrap_or(false))
            .cloned()
            .collect::<Vec<_>>()
    });

    view! {
        <>
            <TopHeader />
            <FloatingDock />
            <div class="mx-auto max-w-2xl space-y-6 pl-24 pr-3 py-6 sm:pl-28 sm:pr-6 sm:py-10">
                {move || if loading.get() {
                    view! {
                        <div class="frosted-card overflow-hidden p-0 animate-fade-in">
                            <div class="aspect-video image-skeleton" />
                        </div>
                    }.into_any()
                } else if video_posts.read().is_empty() {
                    view! {
                        <div class="empty-state-wrap">
                        <div class="frosted-card animate-fade-up p-10 text-center w-full max-w-md">
                            <div class="inline-flex h-16 w-16 items-center justify-center rounded-2xl border border-white/20 bg-white/10 shadow-lg shadow-slate-900/20 backdrop-blur-xl text-accent">
                                <VideoIcon class="h-7 w-7" />
                            </div>
                            <h2 class="mt-5 logo-handwritten text-4xl text-primary sm:text-5xl">
                                "No clips yet"
                            </h2>
                            <p class="mx-auto mt-3 max-w-sm text-sm text-muted">
                                "Record something short, sweet and sovereign. Clips show up here the moment they're posted."
                            </p>
                            <NavLink
                                href="/posts"
                                class="unfrost mt-6 inline-flex items-center gap-2 rounded-full bg-accent px-6 py-2.5 text-sm font-semibold text-accent-text shadow-md transition hover:bg-amber-300"
                            >
                                "Upload a clip"
                            </NavLink>
                        </div>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <For
                            each=move || video_posts.get()
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
        </>
    }
}
