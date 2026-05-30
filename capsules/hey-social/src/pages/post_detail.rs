// PostDetail — single post view, accessed via /post/:id. Rust port of
// capsules/hey-social/client/src/pages/PostDetail.jsx.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_params_map;

use crate::api::posts::{get_post, Post};
use crate::components::PostCard;

#[component]
pub fn PostDetail() -> impl IntoView {
    let params = use_params_map();
    let post: RwSignal<Option<Post>> = RwSignal::new(None);
    let loading = RwSignal::new(true);
    let error = RwSignal::new(String::new());

    Effect::new(move |_| {
        let id = params.read().get("id").map(|s| s.to_string()).unwrap_or_default();
        if id.is_empty() {
            error.set("No post id".into());
            loading.set(false);
            return;
        }
        loading.set(true);
        spawn_local(async move {
            match get_post(&id).await {
                Ok(Some(p)) => {
                    post.set(Some(p));
                }
                Ok(None) => {
                    error.set("Post not found".into());
                }
                Err(e) => {
                    error.set(format!("{e}"));
                }
            }
            loading.set(false);
        });
    });

    view! {
        <>
            <div class="page-enter mx-auto max-w-2xl space-y-4 pl-24 pr-3 py-6 sm:pl-28 sm:pr-6 sm:py-10">
                {move || {
                    if loading.get() {
                        view! {
                            <div class="frosted-card overflow-hidden p-0">
                                <div class="aspect-square image-skeleton skeleton-pulse" />
                            </div>
                        }.into_any()
                    } else if !error.get().is_empty() {
                        view! {
                            <div class="frosted-card p-4 text-sm text-red-400">
                                {error.get()}
                            </div>
                        }.into_any()
                    } else if let Some(p) = post.get() {
                        view! { <div class="animate-fade-up"><PostCard post=p /></div> }.into_any()
                    } else {
                        view! { <></> }.into_any()
                    }
                }}
            </div>
        </>
    }
}
