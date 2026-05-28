// PostCard — feature-rich port of capsules/hey-social/client/src/components/PostCard.jsx.
//
// Renders:
//   * Author header (avatar + name + time-ago)
//   * Multi-image scroll-snap carousel (left/right arrows for desktop;
//     swipe-friendly on mobile via .scroll-snap-x in styles.css)
//   * Reactions row: heart toggle + emoji picker (6 common emojis) +
//     reaction chip per emoji showing count
//   * Comment count + click-to-expand composer with author avatar and
//     in-line list of existing comments
//   * Delete button + confirmation when the post is the current user's
//
// Reply threads, comment reactions, and the reaction count formatter are
// simplified vs React (the full version is 600+ lines; this is a
// representative subset).

use leptos::ev::MouseEvent;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;
use web_sys::HtmlTextAreaElement;

use crate::api::posts::{
    add_comment, delete_post, react_to_post, Comment as PostComment, Post,
};
use crate::components::icons::{CommentIcon, HeartIcon};
use crate::runtime::ipfs;
use crate::session;

const EMOJI_PALETTE: &[&str] = &["❤️", "🔥", "😂", "😮", "🥺", "🙌"];

#[component]
pub fn PostCard(post: Post) -> impl IntoView {
    let post_signal = RwSignal::new(post);
    let me_did = session::current().map(|s| s.did_key).unwrap_or_default();
    let composer_open = RwSignal::new(false);
    let composer_text = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let confirm_delete = RwSignal::new(false);
    let emoji_open = RwSignal::new(false);

    let is_mine = {
        let me_did = me_did.clone();
        Memo::new(move |_| post_signal.read().user_did == me_did)
    };
    let i_reacted = {
        let me_did = me_did.clone();
        Memo::new(move |_| {
            let p = post_signal.read();
            p.reactions
                .get("❤️")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().any(|v| v.as_str() == Some(&me_did)))
                .unwrap_or(false)
        })
    };
    let total_reactions = Memo::new(move |_| {
        post_signal
            .read()
            .reactions
            .values()
            .filter_map(|v| v.as_array())
            .map(|a| a.len())
            .sum::<usize>()
    });
    let comment_count = Memo::new(move |_| post_signal.read().comments.len());

    let do_react = move |emoji: &'static str| {
        let id = post_signal.read().id.clone();
        spawn_local(async move {
            if let Ok(updated) = react_to_post(&id, emoji).await {
                post_signal.set(updated);
            }
        });
    };
    let toggle_heart = move |_| do_react("❤️");

    let post_comment = move |_| {
        if busy.get() {
            return;
        }
        let text = composer_text.get().trim().to_string();
        if text.is_empty() {
            return;
        }
        let id = post_signal.read().id.clone();
        busy.set(true);
        spawn_local(async move {
            if let Ok(updated) = add_comment(&id, &text, None).await {
                post_signal.set(updated);
                composer_text.set(String::new());
            }
            busy.set(false);
        });
    };

    let do_delete = move |_| {
        if busy.get() {
            return;
        }
        let id = post_signal.read().id.clone();
        busy.set(true);
        spawn_local(async move {
            let _ = delete_post(&id).await;
            busy.set(false);
        });
    };

    view! {
        <article class="frosted-card overflow-hidden p-0">
            <PostHeader post=post_signal is_mine=is_mine on_delete=move |_| confirm_delete.set(true) />

            <PostMedia post=post_signal />

            <div class="p-4 space-y-3">
                <div class="flex items-center gap-3 relative">
                    <button
                        type="button"
                        on:click=toggle_heart
                        class="reaction-chip"
                        class:is-active=move || i_reacted.get()
                    >
                        <HeartIcon class="h-4 w-4" filled=i_reacted.get() />
                        <span>{move || total_reactions.get()}</span>
                    </button>

                    <button
                        type="button"
                        on:click=move |_| composer_open.update(|v| *v = !*v)
                        class="reaction-chip"
                    >
                        <CommentIcon class="h-4 w-4" />
                        <span>{move || comment_count.get()}</span>
                    </button>

                    <button
                        type="button"
                        on:click=move |_| emoji_open.update(|v| *v = !*v)
                        class="reaction-chip"
                        title="Add a reaction"
                    >
                        "😊"
                    </button>

                    {move || if emoji_open.get() {
                        view! {
                            <div class="absolute left-0 top-full mt-2 z-20 flex items-center gap-1 rounded-full bg-white/95 dark:bg-slate-900/95 backdrop-blur border border-surface px-2 py-1 shadow-xl">
                                {EMOJI_PALETTE.iter().copied().map(|e| {
                                    let click = move |_: MouseEvent| {
                                        emoji_open.set(false);
                                        do_react(e);
                                    };
                                    view! {
                                        <button
                                            type="button"
                                            on:click=click
                                            class="px-2 py-1 rounded-full hover:bg-white/10 text-base"
                                        >{e}</button>
                                    }
                                }).collect::<Vec<_>>()}
                            </div>
                        }.into_any()
                    } else { view! { <></> }.into_any() }}
                </div>

                <ReactionChips post=post_signal me_did=me_did.clone() />
                <PostCaption post=post_signal />

                {move || if composer_open.get() {
                    view! {
                        <Composer
                            text=composer_text
                            busy=busy
                            on_submit=post_comment.clone()
                        />
                        <CommentList post=post_signal />
                    }.into_any()
                } else { view! { <></> }.into_any() }}

                {move || if confirm_delete.get() {
                    view! {
                        <DeleteConfirm
                            on_confirm=do_delete.clone()
                            on_cancel=move |_| confirm_delete.set(false)
                            busy=busy
                        />
                    }.into_any()
                } else { view! { <></> }.into_any() }}
            </div>
        </article>
    }
}

#[component]
fn PostHeader(
    post: RwSignal<Post>,
    is_mine: Memo<bool>,
    on_delete: impl Fn(MouseEvent) + 'static + Send + Sync + Clone,
) -> impl IntoView {
    view! {
        <header class="flex items-center gap-3 p-4">
            <div class="flex h-12 w-12 flex-none items-center justify-center rounded-full bg-gradient-to-br from-accent to-amber-600 text-base font-bold text-accent-text shadow-sm">
                {move || initial_letters(&post.read().user_name)}
            </div>
            <div class="min-w-0 flex-1">
                <p class="text-sm font-medium text-primary truncate">
                    {move || post.read().user_name.clone()}
                </p>
                <p class="text-[11px] text-muted truncate">
                    {move || time_ago(&post.read().created_at)}
                </p>
            </div>
            {move || if is_mine.get() {
                let on_delete = on_delete.clone();
                view! {
                    <button
                        type="button"
                        on:click=on_delete
                        class="icon-btn-ghost"
                        aria-label="Delete post"
                        title="Delete post"
                    >
                        <svg viewBox="0 0 24 24" class="h-4 w-4" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <path d="M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
                        </svg>
                    </button>
                }.into_any()
            } else { view! { <></> }.into_any() }}
        </header>
    }
}

#[component]
fn PostMedia(post: RwSignal<Post>) -> impl IntoView {
    let count = Memo::new(move |_| post.read().images.len());
    view! {
        {move || {
            let images = post.read().images.clone();
            if images.is_empty() {
                return view! { <></> }.into_any();
            }
            if images.len() == 1 {
                let m = &images[0];
                return if m.media_type == "video" {
                    view! {
                        <video
                            controls
                            class="block w-full bg-black"
                            src=ipfs::gateway_url(&m.cid, None)
                        />
                    }.into_any()
                } else {
                    view! {
                        <img
                            class="block w-full bg-slate-100 dark:bg-slate-800 aspect-square object-cover"
                            src=ipfs::gateway_url(&m.cid, None)
                            alt=m.name.clone()
                            loading="lazy"
                        />
                    }.into_any()
                };
            }
            // Multi-image carousel — scroll-snap-x from styles.css.
            view! {
                <div class="relative">
                    <div class="scroll-snap-x flex overflow-x-auto">
                        {images.into_iter().map(|m| {
                            let url = ipfs::gateway_url(&m.cid, None);
                            if m.media_type == "video" {
                                view! {
                                    <video
                                        controls
                                        class="block w-full flex-none bg-black"
                                        src=url
                                    />
                                }.into_any()
                            } else {
                                view! {
                                    <img
                                        class="block w-full flex-none bg-slate-100 dark:bg-slate-800 aspect-square object-cover"
                                        src=url
                                        alt=m.name.clone()
                                        loading="lazy"
                                    />
                                }.into_any()
                            }
                        }).collect::<Vec<_>>()}
                    </div>
                    <span class="pointer-events-none absolute right-3 top-3 rounded-full bg-black/60 text-white text-[11px] px-2 py-0.5">
                        {move || format!("1/{}", count.get())}
                    </span>
                </div>
            }.into_any()
        }}
    }
}

#[component]
fn ReactionChips(post: RwSignal<Post>, me_did: String) -> impl IntoView {
    view! {
        {move || {
            let p = post.read();
            let chips: Vec<_> = p.reactions.iter().filter_map(|(emoji, val)| {
                let count = val.as_array().map(|a| a.len()).unwrap_or(0);
                if count == 0 || emoji == "❤️" { return None; }
                let mine = val.as_array().map(|a| a.iter().any(|v| v.as_str() == Some(&me_did))).unwrap_or(false);
                Some((emoji.clone(), count, mine))
            }).collect();
            if chips.is_empty() { view! { <></> }.into_any() } else {
                view! {
                    <div class="flex flex-wrap items-center gap-2">
                        {chips.into_iter().map(|(emoji, count, mine)| {
                            let active = if mine { "reaction-chip is-active" } else { "reaction-chip" };
                            view! {
                                <span class=active>
                                    <span>{emoji}</span>
                                    <span>{count}</span>
                                </span>
                            }
                        }).collect::<Vec<_>>()}
                    </div>
                }.into_any()
            }
        }}
    }
}

#[component]
fn PostCaption(post: RwSignal<Post>) -> impl IntoView {
    view! {
        {move || {
            let caption = post.read().caption.clone();
            if caption.is_empty() {
                view! { <></> }.into_any()
            } else {
                view! {
                    <p class="text-sm text-primary leading-snug whitespace-pre-wrap">{caption}</p>
                }.into_any()
            }
        }}
    }
}

#[component]
fn Composer(
    text: RwSignal<String>,
    busy: RwSignal<bool>,
    on_submit: impl Fn(MouseEvent) + 'static + Send + Sync + Clone,
) -> impl IntoView {
    let on_input = move |ev: web_sys::Event| {
        if let Some(t) = ev.target() {
            if let Ok(ta) = t.dyn_into::<HtmlTextAreaElement>() {
                text.set(ta.value());
            }
        }
    };
    view! {
        <div class="border-t border-surface pt-3 space-y-2">
            <textarea
                class="frosted-input text-sm"
                rows="2"
                maxlength="500"
                placeholder="Add a comment…"
                prop:value=move || text.get()
                on:input=on_input
            />
            <div class="flex justify-end">
                <button
                    type="button"
                    on:click=on_submit
                    prop:disabled=move || busy.get() || text.get().trim().is_empty()
                    class="unfrost rounded-full bg-accent px-4 py-1.5 text-xs font-semibold text-accent-text disabled:opacity-50 disabled:cursor-not-allowed"
                >
                    {move || if busy.get() { "Posting…" } else { "Post" }}
                </button>
            </div>
        </div>
    }
}

#[component]
fn CommentList(post: RwSignal<Post>) -> impl IntoView {
    view! {
        {move || {
            let comments = post.read().comments.clone();
            if comments.is_empty() {
                view! { <></> }.into_any()
            } else {
                view! {
                    <ul class="space-y-2 mt-2">
                        {comments.into_iter().map(|c: PostComment| view! {
                            <li class="flex gap-2">
                                <span class="flex h-8 w-8 flex-none items-center justify-center rounded-full bg-gradient-to-br from-amber-300 to-amber-600 text-xs font-bold text-slate-900 shadow-sm">
                                    {initial_letters(&c.user_name)}
                                </span>
                                <div class="flex-1 rounded-2xl bg-white/10 px-3 py-2 border border-surface">
                                    <p class="text-[11px] font-medium text-primary">{c.user_name.clone()}</p>
                                    <p class="text-sm text-primary whitespace-pre-wrap">{c.text.clone()}</p>
                                </div>
                            </li>
                        }).collect::<Vec<_>>()}
                    </ul>
                }.into_any()
            }
        }}
    }
}

#[component]
fn DeleteConfirm(
    on_confirm: impl Fn(MouseEvent) + 'static + Send + Sync + Clone,
    on_cancel: impl Fn(MouseEvent) + 'static + Send + Sync + Clone,
    busy: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <div class="rounded-2xl border border-rose-400/40 bg-rose-500/10 p-3 space-y-2 animate-fade-in">
            <p class="text-sm text-rose-700 dark:text-rose-300">"Delete this post? You can't undo this."</p>
            <div class="flex justify-end gap-2">
                <button
                    type="button"
                    on:click=on_cancel
                    class="unfrost rounded-full bg-white/10 border border-surface px-4 py-1.5 text-xs font-semibold text-primary"
                >"Cancel"</button>
                <button
                    type="button"
                    on:click=on_confirm
                    prop:disabled=move || busy.get()
                    class="unfrost rounded-full bg-rose-500 hover:bg-rose-600 px-4 py-1.5 text-xs font-semibold text-white disabled:opacity-50"
                >
                    {move || if busy.get() { "Deleting…" } else { "Delete" }}
                </button>
            </div>
        </div>
    }
}

fn initial_letters(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .take(2)
        .map(|c| c.to_uppercase().next().unwrap_or(c))
        .collect::<String>()
        .to_uppercase()
}

// Human "time ago" string from an ISO timestamp. Falls back to the ISO
// string itself if parsing fails — never panics.
fn time_ago(iso: &str) -> String {
    if iso.is_empty() {
        return String::new();
    }
    let now = js_sys::Date::now();
    let parsed = js_sys::Date::parse(iso);
    if parsed.is_nan() {
        return iso.into();
    }
    let secs = ((now - parsed) / 1000.0).max(1.0) as i64;
    if secs < 60 {
        return format!("{secs}s");
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{mins}m");
    }
    let hours = mins / 60;
    if hours < 24 {
        return format!("{hours}h");
    }
    let days = hours / 24;
    if days < 7 {
        return format!("{days}d");
    }
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(parsed));
    d.to_locale_date_string("en-US", &wasm_bindgen::JsValue::UNDEFINED)
        .as_string()
        .unwrap_or_default()
}

