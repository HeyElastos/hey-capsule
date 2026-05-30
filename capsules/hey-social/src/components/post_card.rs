// PostCard — feature-rich port of capsules/hey-social/client/src/components/PostCard.jsx.
//
// Renders:
//   * Author header (avatar + name + time-ago)
//   * Multi-image CAROUSEL: one photo at a time, switched by prev/next
//     arrows, horizontal pointer-drag, and tappable dot indicators
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
    add_comment, delete_comment, delete_post, react_to_post, Comment as PostComment, Post,
};
use crate::components::icons::{CommentIcon, HeartIcon};
use crate::components::Modal;
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
    // Physical press feedback for the heart button (motion-only).
    let press = RwSignal::new(false);
    // Local "this card was deleted" flag — hides the card immediately on a
    // successful delete (the post lingers in the parent's list until refetch,
    // so without this the deleted post stays visible).
    let deleted = RwSignal::new(false);

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

    // Crisp count-change pulse: flip `count_pulse` on each total change, then
    // self-clear after the chip-confirm animation so it can re-fire.
    let count_pulse = RwSignal::new(false);
    Effect::new(move |prev: Option<usize>| {
        let now = total_reactions.get();
        if let Some(was) = prev {
            if was != now {
                count_pulse.set(true);
                spawn_local(async move {
                    crate::runtime::sleep_ms(400i32).await;
                    count_pulse.set(false);
                });
            }
        }
        now
    });

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
            let ok = delete_post(&id).await.is_ok();
            busy.set(false);
            confirm_delete.set(false);
            if ok {
                deleted.set(true);
            }
        });
    };

    view! {
        <article
            class="frosted-card overflow-hidden p-0"
            style=move || if deleted.get() { "display:none".to_string() } else { String::new() }
        >
            <PostHeader post=post_signal is_mine=is_mine on_delete=move |_| confirm_delete.set(true) />

            <PostMedia post=post_signal />

            <div class="p-4 space-y-3">
                <div class="flex items-center gap-3 relative">
                    <button
                        type="button"
                        on:click=toggle_heart
                        on:mousedown=move |_: MouseEvent| press.set(true)
                        on:mouseup=move |_: MouseEvent| press.set(false)
                        on:mouseleave=move |_: MouseEvent| press.set(false)
                        class="reaction-chip"
                        class:is-active=move || i_reacted.get()
                        class:btn-press=move || press.get()
                    >
                        {move || view! {
                            <span class="inline-flex" class:heart-fill=move || i_reacted.get()>
                                <HeartIcon class="h-4 w-4" filled=i_reacted.get() />
                            </span>
                        }}
                        <span class:chip-confirm=move || count_pulse.get()>
                            {move || total_reactions.get()}
                        </span>
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
                            <div class="chip-confirm absolute left-0 top-full mt-2 z-20 flex items-center gap-1 rounded-full bg-white/95 dark:bg-slate-900/95 backdrop-blur border border-surface px-2 py-1 shadow-xl">
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
                    }.into_any()
                } else { view! { <></> }.into_any() }}

                // Existing comments ALWAYS show (renders nothing when empty);
                // the comment button only toggles the Composer for ADDING one.
                <CommentList post=post_signal me_did=me_did.clone() />

                <DeleteConfirm
                    open=confirm_delete
                    on_confirm=do_delete
                    busy=busy
                />
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
    // Post images are IMMUTABLE after creation, so read them ONCE,
    // non-reactively. If we read them inside a `move ||` closure that tracks
    // `post`, every `post_signal.set(...)` from a react/comment would re-run
    // it and REBUILD the Carousel, resetting its `active` index to 0. Reading
    // untracked here means the Carousel (and its `active`) survives reactions.
    let images = post.get_untracked().images;

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
    // Multi-image CAROUSEL: one slide at a time, switched by arrows,
    // pointer-drag, or dot taps. `active` is the current slide index;
    // the flex track is translated by `active * 100%`.
    view! { <Carousel images=images /> }.into_any()
}

/// One-photo-at-a-time carousel for multi-image posts. Switches slides via
/// prev/next arrows, horizontal pointer-drag (threshold), and tappable dots.
#[component]
fn Carousel(images: Vec<crate::api::posts::MediaTile>) -> impl IntoView {
    let len = images.len();
    // Current slide index.
    let active = RwSignal::new(0usize);
    // Live horizontal drag offset in px (finger/mouse follow). 0 when idle.
    let drag_dx = RwSignal::new(0i32);
    // True while a pointer-drag is in progress.
    let dragging = RwSignal::new(false);
    // Pointer-drag start x (in client px); None when no drag is in progress.
    let drag_start = RwSignal::new(None::<i32>);
    // Swipe distance (px) past which the slide advances on release.
    const THRESHOLD: i32 = 50;

    let go_prev = move |_| active.update(|i| if *i > 0 { *i -= 1; });
    let go_next = move |_| active.update(|i| if *i + 1 < len { *i += 1; });

    let on_down = move |ev: web_sys::PointerEvent| {
        drag_start.set(Some(ev.client_x()));
        dragging.set(true);
        drag_dx.set(0);
    };
    let on_move = move |ev: web_sys::PointerEvent| {
        if dragging.get() {
            if let Some(start) = drag_start.get() {
                drag_dx.set(ev.client_x() - start);
            }
        }
    };
    let on_up = move |ev: web_sys::PointerEvent| {
        if let Some(start) = drag_start.get() {
            let delta = ev.client_x() - start;
            if delta < -THRESHOLD {
                active.update(|i| if *i + 1 < len { *i += 1; });
            } else if delta > THRESHOLD {
                active.update(|i| if *i > 0 { *i -= 1; });
            }
        }
        drag_start.set(None);
        dragging.set(false);
        drag_dx.set(0);
    };
    let on_leave = move |_: web_sys::PointerEvent| {
        drag_start.set(None);
        dragging.set(false);
        drag_dx.set(0);
    };

    // Track transform: slide offset + live finger-follow. Built in a closure
    // (NO angle-brackets in the view! macro). The CSS transition is disabled
    // inline while dragging (1:1 follow) and re-enabled on release so the snap
    // animates — done via inline style since styles.css is prebuilt and we
    // can't add a no-transition utility class to it.
    let track_style = move || {
        let transition = if dragging.get() {
            "transition: none".to_string()
        } else {
            "transition: transform 0.3s cubic-bezier(0,0,0.2,1)".to_string()
        };
        format!(
            "transform: translateX(calc(-{}% + {}px)); {}",
            active.get() * 100,
            drag_dx.get(),
            transition
        )
    };

    // Arrow visibility closures built OUTSIDE view! — they use `==`/comparison
    // operators (the `>=` would be parsed as a tag by the view! macro). Hide via
    // opacity + pointer-events (NOT display:none) so the arrow stays at a stable
    // absolute position and never reflows on hover.
    let at_first = move || active.get() == 0;
    let at_last = move || active.get() + 1 >= len;
    let prev_style = move || if at_first() {
        "opacity:0; pointer-events:none".to_string()
    } else {
        "opacity:1".to_string()
    };
    let next_style = move || if at_last() {
        "opacity:0; pointer-events:none".to_string()
    } else {
        "opacity:1".to_string()
    };

    view! {
        <div class="relative overflow-hidden select-none">
            // Sliding track: each slide is full width; translate to show `active`
            // plus the live drag offset. The inline transition (toggled off while
            // dragging) makes the photo track the finger 1:1, then snap-animate.
            <div
                class="flex"
                style=track_style
                on:pointerdown=on_down
                on:pointermove=on_move
                on:pointerup=on_up
                on:pointerleave=on_leave
            >
                {images.into_iter().map(|m| {
                    let url = ipfs::gateway_url(&m.cid, None);
                    view! {
                        <div class="w-full flex-none">
                            {if m.media_type == "video" {
                                view! {
                                    <video
                                        controls
                                        class="block w-full bg-black"
                                        src=url
                                    />
                                }.into_any()
                            } else {
                                view! {
                                    <img
                                        class="block w-full bg-slate-100 dark:bg-slate-800 aspect-square object-cover"
                                        src=url
                                        alt=m.name.clone()
                                        loading="lazy"
                                        draggable="false"
                                    />
                                }.into_any()
                            }}
                        </div>
                    }
                }).collect::<Vec<_>>()}
            </div>

            // PREV arrow — kept at a STABLE absolute position. At the first
            // slide it fades out via opacity/pointer-events (NOT display:none),
            // so hovering never reflows or shifts it.
            <button
                type="button"
                on:click=go_prev
                aria-label="Previous photo"
                class="absolute left-3 top-1/2 -translate-y-1/2 z-10 flex h-9 w-9 items-center justify-center rounded-full bg-black/45 hover:bg-black/75 text-white backdrop-blur shadow-lg transition-colors"
                style=prev_style
            >
                <svg viewBox="0 0 24 24" class="h-5 w-5" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                    <path d="M15 18l-6-6 6-6" />
                </svg>
            </button>

            // NEXT arrow — same stable-position treatment as PREV.
            <button
                type="button"
                on:click=go_next
                aria-label="Next photo"
                class="absolute right-3 top-1/2 -translate-y-1/2 z-10 flex h-9 w-9 items-center justify-center rounded-full bg-black/45 hover:bg-black/75 text-white backdrop-blur shadow-lg transition-colors"
                style=next_style
            >
                <svg viewBox="0 0 24 24" class="h-5 w-5" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                    <path d="M9 6l6 6-6 6" />
                </svg>
            </button>

            // Position counter badge — updates with `active`.
            <span class="absolute right-3 top-3 z-10 pointer-events-none rounded-full bg-black/45 text-white backdrop-blur text-[11px] font-semibold px-2 py-0.5 shadow">
                {move || format!("{} / {}", active.get() + 1, len)}
            </span>

            // Dot indicator — one per slide, the active one filled; tap to jump.
            <div class="absolute bottom-3 left-1/2 -translate-x-1/2 z-10 flex items-center gap-1.5">
                {(0..len).map(|idx| view! {
                    <button
                        type="button"
                        on:click=move |_| active.set(idx)
                        aria-label=format!("Go to photo {}", idx + 1)
                        class="h-2 w-2 rounded-full transition-colors cursor-pointer"
                        class:bg-white=move || active.get() == idx
                        style=move || if active.get() == idx {
                            String::new()
                        } else {
                            "background: rgba(255,255,255,0.45)".to_string()
                        }
                    />
                }).collect::<Vec<_>>()}
            </div>
        </div>
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
fn CommentList(post: RwSignal<Post>, me_did: String) -> impl IntoView {
    view! {
        {move || {
            let comments = post.read().comments.clone();
            if comments.is_empty() {
                return view! { <></> }.into_any();
            }
            // Group by parent: top-level = parent_id None; replies hang off
            // their parent's id. Render replies INDENTED under their parent.
            let top_level: Vec<PostComment> = comments
                .iter()
                .filter(|c| c.parent_id.is_none())
                .cloned()
                .collect();
            view! {
                <ul class="space-y-3 mt-2">
                    {top_level.into_iter().map(|c| {
                        let parent_id = c.id.clone();
                        let replies: Vec<PostComment> = comments
                            .iter()
                            .filter(|r| r.parent_id.as_deref() == Some(&parent_id))
                            .cloned()
                            .collect();
                        let me_did = me_did.clone();
                        let reply_me_did = me_did.clone();
                        view! {
                            <li>
                                <CommentRow post=post comment=c.clone() me_did=me_did />
                                {if replies.is_empty() {
                                    view! { <></> }.into_any()
                                } else {
                                    let me_did = reply_me_did.clone();
                                    view! {
                                        <ul class="space-y-3 mt-2 border-l border-surface pl-3" style="margin-left:2.5rem">
                                            {replies.into_iter().map(|r| {
                                                view! {
                                                    <li>
                                                        <CommentRow post=post comment=r me_did=me_did.clone() />
                                                    </li>
                                                }
                                            }).collect::<Vec<_>>()}
                                        </ul>
                                    }.into_any()
                                }}
                            </li>
                        }
                    }).collect::<Vec<_>>()}
                </ul>
            }.into_any()
        }}
    }
}

/// One comment bubble + its inline affordances: a Reply toggle (opens a tiny
/// composer that calls add_comment with this comment as parent) and, for the
/// author, a hover-revealed delete. Replies to replies aren't nested further
/// (one level of indentation) — add_comment is called with the top-level
/// parent id passed in via `reply_parent`.
#[component]
fn CommentRow(post: RwSignal<Post>, comment: PostComment, me_did: String) -> impl IntoView {
    let comment_id = comment.id.clone();
    // A reply targets this comment's own id, so it groups under it. (If this
    // row is itself a reply, replying still groups under the same top-level
    // thread via its parent_id chain.)
    let reply_parent = comment
        .parent_id
        .clone()
        .unwrap_or_else(|| comment.id.clone());
    let is_mine = comment.user_did == me_did;
    let user_name = comment.user_name.clone();
    let text = comment.text.clone();

    let reply_open = RwSignal::new(false);
    let reply_text = RwSignal::new(String::new());
    let busy = RwSignal::new(false);

    let on_reply_input = move |ev: web_sys::Event| {
        if let Some(t) = ev.target() {
            if let Ok(ta) = t.dyn_into::<HtmlTextAreaElement>() {
                reply_text.set(ta.value());
            }
        }
    };

    let submit_reply = {
        let reply_parent = reply_parent.clone();
        move |_: MouseEvent| {
            if busy.get() {
                return;
            }
            let body = reply_text.get().trim().to_string();
            if body.is_empty() {
                return;
            }
            let pid = post.read().id.clone();
            let parent = reply_parent.clone();
            busy.set(true);
            spawn_local(async move {
                if let Ok(updated) = add_comment(&pid, &body, Some(parent)).await {
                    post.set(updated);
                    reply_text.set(String::new());
                    reply_open.set(false);
                }
                busy.set(false);
            });
        }
    };

    let do_delete = {
        let comment_id = comment_id.clone();
        move |_: MouseEvent| {
            if busy.get() {
                return;
            }
            let pid = post.read().id.clone();
            let cid = comment_id.clone();
            busy.set(true);
            spawn_local(async move {
                if let Ok(updated) = delete_comment(&pid, &cid).await {
                    post.set(updated);
                }
                busy.set(false);
            });
        }
    };

    view! {
        <div class="group flex gap-2">
            <span class="flex h-8 w-8 flex-none items-center justify-center rounded-full bg-gradient-to-br from-amber-300 to-amber-600 text-xs font-bold text-slate-900 shadow-sm">
                {initial_letters(&user_name)}
            </span>
            <div class="flex-1 min-w-0">
                <div class="comment-bubble">
                    <p class="text-[11px] font-medium text-primary">{user_name}</p>
                    <p class="text-sm text-primary whitespace-pre-wrap">{text}</p>
                </div>
                <div class="flex items-center gap-3 mt-1" style="padding-left:0.25rem">
                    <button
                        type="button"
                        on:click=move |_| reply_open.update(|v| *v = !*v)
                        class="text-[11px] font-medium text-muted hover:text-primary transition-colors"
                    >
                        "Reply"
                    </button>
                    {if is_mine {
                        let do_delete = do_delete.clone();
                        view! {
                            <button
                                type="button"
                                on:click=do_delete
                                prop:disabled=move || busy.get()
                                aria-label="Delete comment"
                                title="Delete comment"
                                class="text-[11px] font-medium text-muted hover:text-rose-500 dark:hover:text-rose-400 transition-colors opacity-0 group-hover:opacity-100"
                            >
                                "Delete"
                            </button>
                        }.into_any()
                    } else {
                        view! { <></> }.into_any()
                    }}
                </div>
                {move || if reply_open.get() {
                    let submit_reply = submit_reply.clone();
                    view! {
                        <div class="mt-2 space-y-2">
                            <textarea
                                class="frosted-input text-sm"
                                rows="2"
                                maxlength="500"
                                placeholder="Write a reply…"
                                prop:value=move || reply_text.get()
                                on:input=on_reply_input
                            />
                            <div class="flex justify-end gap-2">
                                <button
                                    type="button"
                                    on:click=move |_| reply_open.set(false)
                                    class="rounded-full bg-white/10 border border-surface text-primary text-xs font-semibold px-3 py-1.5 hover:bg-white/15 transition-colors"
                                >
                                    "Cancel"
                                </button>
                                <button
                                    type="button"
                                    on:click=submit_reply
                                    prop:disabled=move || busy.get() || reply_text.get().trim().is_empty()
                                    class="unfrost rounded-full bg-accent px-3 py-1.5 text-xs font-semibold text-accent-text disabled:opacity-50 disabled:cursor-not-allowed"
                                >
                                    {move || if busy.get() { "Replying…" } else { "Reply" }}
                                </button>
                            </div>
                        </div>
                    }.into_any()
                } else { view! { <></> }.into_any() }}
            </div>
        </div>
    }
}

#[component]
fn DeleteConfirm(
    open: RwSignal<bool>,
    on_confirm: impl Fn(MouseEvent) + 'static + Send + Sync + Clone,
    busy: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <Modal open=open>
            <div class="frosted-card frosted-card-strong p-5 space-y-3">
                <header class="flex items-center justify-between">
                    <h3 class="logo-handwritten text-4xl text-primary">"Delete post"</h3>
                    <button
                        type="button"
                        on:click=move |_| open.set(false)
                        class="icon-btn-ghost"
                        aria-label="Close"
                    >
                        <svg viewBox="0 0 24 24" class="h-4 w-4" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <path d="M18 6 6 18M6 6l12 12" />
                        </svg>
                    </button>
                </header>

                <p class="text-sm text-rose-600 dark:text-rose-300">
                    "Delete this post? You can\u{2019}t undo this."
                </p>

                <div class="flex gap-2 pt-2">
                    <button
                        type="button"
                        on:click=move |_| open.set(false)
                        class="flex-1 rounded-full bg-white/10 border border-surface text-primary text-sm font-semibold px-4 py-2.5 hover:bg-white/15 transition-colors"
                    >
                        "Cancel"
                    </button>
                    <button
                        type="button"
                        on:click=on_confirm.clone()
                        prop:disabled=move || busy.get()
                        class="flex-1 unfrost rounded-full bg-rose-500 hover:bg-rose-600 text-white font-semibold px-4 py-2.5 text-sm transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                        {move || if busy.get() { "Deleting…" } else { "Delete" }}
                    </button>
                </div>
            </div>
        </Modal>
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

