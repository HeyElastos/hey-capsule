// Profile — view + edit user profile. Rust port of capsules/hey-social/
// client/src/pages/Profile.jsx (664 lines of React; this is a focused
// subset: identity card, edit name/bio, photo grid of own posts).
//
// Profile editing writes to BOTH the Hey-local profile.json AND the
// shared identity at .AppData/{ElastOS,}/Identity/profile.json — see
// api::profile::update_profile for the dual-write semantics.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_params_map;
use wasm_bindgen::JsCast;
use web_sys::HtmlInputElement;

use wasm_bindgen_futures::JsFuture;
use web_sys::Event;

use crate::api::dms::invite_qr_svg;
use crate::api::posts::{delete_post, get_user_posts, Post};
use crate::api::profile::{
    ensure_profile, follow_user, is_following, unfollow_user, update_profile, upload_avatar,
    Profile as ProfileRecord, ProfileUpdate,
};
use crate::components::icons::{CommentIcon, HeartIcon};
use crate::components::{Modal, NavLink};
use crate::runtime::ipfs;
use crate::session;

// Render a did:key truncated for the identity row: first ~16 chars + … + last 6.
fn truncate_did(did: &str) -> String {
    let chars: Vec<char> = did.chars().collect();
    if chars.len() <= 26 {
        return did.to_string();
    }
    let head: String = chars.iter().take(16).collect();
    let tail: String = chars.iter().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect();
    format!("{head}…{tail}")
}

// Count of all reactions on a post (sum of each emoji's reactor list).
fn reaction_count(p: &Post) -> usize {
    p.reactions
        .values()
        .map(|v| v.as_array().map(|a| a.len()).unwrap_or(0))
        .sum()
}

#[component]
pub fn Profile() -> impl IntoView {
    let params = use_params_map();
    let profile: RwSignal<Option<ProfileRecord>> = RwSignal::new(None);
    let posts: RwSignal<Vec<Post>> = RwSignal::new(Vec::new());
    let editing = RwSignal::new(false);
    let edit_name = RwSignal::new(String::new());
    let edit_bio = RwSignal::new(String::new());
    let saving = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let following = RwSignal::new(false);
    let avatar_busy = RwSignal::new(false);
    // DID actions: a frosted QR popup + a transient "Copied" affordance.
    let qr_open = RwSignal::new(false);
    let copied = RwSignal::new(false);

    Effect::new(move |_| {
        let did_param = params
            .read()
            .get("did")
            .map(|s| s.to_string())
            .unwrap_or_default();
        let me_did = session::current().map(|s| s.did_key).unwrap_or_default();
        spawn_local(async move {
            // For "me" we ensure-and-backfill; for anyone else we render
            // best-effort from get_user_posts + a follow-status probe.
            if did_param.is_empty() || did_param == me_did {
                if let Ok(me) = ensure_profile().await {
                    edit_name.set(me.name.clone());
                    edit_bio.set(me.bio.clone());
                    profile.set(Some(me));
                }
            } else {
                // Probe local follow state.
                following.set(is_following(&did_param).await);
            }
            let target = if did_param.is_empty() {
                me_did.clone()
            } else {
                did_param.clone()
            };
            if !target.is_empty() {
                let p = get_user_posts(&target).await.unwrap_or_default();
                posts.set(p);
            }
        });
    });

    let on_name_input = move |ev: web_sys::Event| {
        if let Some(target) = ev.target() {
            if let Ok(input) = target.dyn_into::<HtmlInputElement>() {
                edit_name.set(input.value());
            }
        }
    };
    let on_bio_input = move |ev: web_sys::Event| {
        if let Some(target) = ev.target() {
            if let Ok(ta) = target.dyn_into::<web_sys::HtmlTextAreaElement>() {
                edit_bio.set(ta.value());
            }
        }
    };

    let save = move |_| {
        if saving.get() {
            return;
        }
        let name = edit_name.get();
        let bio = edit_bio.get();
        saving.set(true);
        error.set(String::new());
        spawn_local(async move {
            match update_profile(ProfileUpdate {
                name: Some(name),
                bio: Some(bio),
                avatar: None,
            })
            .await
            {
                Ok(p) => {
                    profile.set(Some(p));
                    editing.set(false);
                }
                Err(e) => error.set(format!("{e}")),
            }
            saving.set(false);
        });
    };

    let me_did = session::current().map(|s| s.did_key).unwrap_or_default();
    let is_self_view = Memo::new({
        let me_did = me_did.clone();
        move |_| {
            let p = params.read();
            let did_param = p.get("did").unwrap_or_default();
            did_param.is_empty() || did_param == me_did
        }
    });

    let on_follow_click = move |_| {
        let did = params
            .read()
            .get("did")
            .map(|s| s.to_string())
            .unwrap_or_default();
        if did.is_empty() {
            return;
        }
        let want_follow = !following.get();
        spawn_local(async move {
            let result = if want_follow {
                follow_user(&did).await
            } else {
                unfollow_user(&did).await
            };
            if result.is_ok() {
                following.set(want_follow);
            }
        });
    };

    let on_avatar_change = move |ev: Event| {
        let target = ev.target().unwrap();
        let input: HtmlInputElement = target.dyn_into().unwrap();
        let Some(files) = input.files() else { return };
        if files.length() == 0 {
            return;
        }
        let file = files.get(0).unwrap();
        let name = file.name();
        let mime = file.type_();
        avatar_busy.set(true);
        spawn_local(async move {
            let buf_value = match JsFuture::from(file.array_buffer()).await {
                Ok(v) => v,
                Err(_) => {
                    avatar_busy.set(false);
                    return;
                }
            };
            let array = js_sys::Uint8Array::new(&buf_value);
            let mut bytes = vec![0u8; array.length() as usize];
            array.copy_to(&mut bytes);
            if let Ok(p) = upload_avatar(&bytes, &name, &mime).await {
                profile.set(Some(p));
            }
            avatar_busy.set(false);
        });
    };

    // Copy the FULL did:key to the clipboard with a brief "Copied" state.
    let on_copy_did = move |_| {
        let did = profile.get().map(|p| p.did_key).unwrap_or_default();
        if did.is_empty() {
            return;
        }
        if let Some(win) = web_sys::window() {
            let clipboard = win.navigator().clipboard();
            let _ = clipboard.write_text(&did);
        } else if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            // execCommand fallback for sandboxes without the async clipboard API.
            let _ = doc;
        }
        copied.set(true);
        // Auto-clear the "Copied" affordance after a moment.
        spawn_local(async move {
            crate::runtime::sleep_ms(1400).await;
            copied.set(false);
        });
    };

    view! {
        <>
            // Chrome (TopHeader + FloatingDock) now lives at the App level
            // (lib.rs AppChrome) so it persists across navigation — this page
            // renders only its own content.
            <div class="page-enter mx-auto max-w-2xl space-y-6 pl-24 pr-3 pt-10 pb-6 sm:pl-28 sm:pr-6 sm:pt-14 sm:pb-10">
                <header class="frosted-card p-6 sm:p-8 animate-fade-up">
                    {move || match profile.get() {
                        Some(me) => {
                            let me_avatar = me.clone();
                            let me_did_row = me.clone();
                            let me_actions = me.clone();
                            view! {
                            <div class="flex flex-col items-center gap-5 sm:flex-row sm:items-start sm:gap-6">
                                <label class="relative h-24 w-24 flex-none cursor-pointer">
                                    {if me_avatar.avatar.is_empty() {
                                        view! {
                                            <div class="h-24 w-24 rounded-full bg-gradient-to-br from-accent to-amber-600 grid place-items-center text-accent-text text-3xl font-bold shadow-sm">
                                                {me_avatar.name.chars().next().map(|c| c.to_uppercase().next().unwrap_or(c).to_string()).unwrap_or_else(|| "?".into())}
                                            </div>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <img src=me_avatar.avatar.clone() alt="" class="h-24 w-24 rounded-full object-cover ring-1 ring-white/15 shadow-sm" />
                                        }.into_any()
                                    }}
                                    {move || if is_self_view.get() {
                                        view! {
                                            <input
                                                type="file"
                                                class="sr-only"
                                                accept="image/*"
                                                on:change=on_avatar_change.clone()
                                            />
                                            <span class="absolute bottom-0 right-0 inline-flex h-8 w-8 items-center justify-center rounded-full bg-accent text-accent-text text-base shadow-md ring-2 ring-white/80 dark:ring-slate-900/80">
                                                {move || if avatar_busy.get() {
                                                    view! {
                                                        <svg viewBox="0 0 24 24" class="spinner h-4 w-4" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" aria-hidden="true">
                                                            <path d="M21 12a9 9 0 1 1-6.2-8.5" />
                                                        </svg>
                                                    }.into_any()
                                                } else {
                                                    view! { "+" }.into_any()
                                                }}
                                            </span>
                                        }.into_any()
                                    } else { view! { <></> }.into_any() }}
                                </label>
                                <div class="min-w-0 flex-1 text-center sm:text-left">
                                    {move || if editing.get() && is_self_view.get() {
                                        view! {
                                            <Modal open=editing>
                                            <div class="frosted-card frosted-card-strong p-6 space-y-4 text-left">
                                            <h3 class="text-lg font-bold text-white">"Edit profile"</h3>
                                                <label class="block">
                                                    <span class="mb-1 block text-xs font-semibold uppercase tracking-wider text-white/70">"Nickname"</span>
                                                    <input
                                                        class="edit-field"
                                                        type="text"
                                                        maxlength="30"
                                                        placeholder="Your nickname"
                                                        prop:value=move || edit_name.get()
                                                        on:input=on_name_input
                                                    />
                                                </label>
                                                <label class="block">
                                                    <span class="mb-1 block text-xs font-semibold uppercase tracking-wider text-white/70">"Bio"</span>
                                                    <textarea
                                                        class="edit-field"
                                                        rows="3"
                                                        maxlength="280"
                                                        placeholder="Say something about yourself…"
                                                        prop:value=move || edit_bio.get()
                                                        on:input=on_bio_input
                                                    />
                                                </label>
                                                {move || {
                                                    let m = error.get();
                                                    if m.is_empty() { view! { <></> }.into_any() }
                                                    else { view! { <p class="text-xs text-rose-500">{m}</p> }.into_any() }
                                                }}
                                                <div class="flex gap-2">
                                                    <button
                                                        type="button"
                                                        on:click=save
                                                        prop:disabled=move || saving.get()
                                                        class="inline-flex items-center gap-1.5 rounded-full bg-amber-500 hover:bg-amber-600 disabled:bg-slate-300 dark:disabled:bg-slate-700 text-white font-semibold px-4 py-2 text-xs transition-colors"
                                                        class:saving=move || saving.get()
                                                    >
                                                        {move || if saving.get() {
                                                            view! {
                                                                <svg viewBox="0 0 24 24" class="spinner h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" aria-hidden="true">
                                                                    <path d="M21 12a9 9 0 1 1-6.2-8.5" />
                                                                </svg>
                                                                "Saving…"
                                                            }.into_any()
                                                        } else {
                                                            view! { "Save" }.into_any()
                                                        }}
                                                    </button>
                                                    <button
                                                        type="button"
                                                        on:click=move |_| { editing.set(false); }
                                                        class="rounded-full bg-slate-100 dark:bg-slate-800 text-slate-700 dark:text-slate-300 font-semibold px-4 py-2 text-xs"
                                                    >
                                                        "Cancel"
                                                    </button>
                                                </div>
                                            </div>
                                            </Modal>
                                        }.into_any()
                                    } else {
                                        let me_view = me.clone();
                                        view! {
                                            <>
                                                <h1 class="text-2xl font-bold text-white truncate">
                                                    {me_view.name.clone()}
                                                </h1>
                                                {if me_view.bio.is_empty() {
                                                    view! { <></> }.into_any()
                                                } else {
                                                    view! {
                                                        <p class="mt-2 text-sm text-slate-700 dark:text-slate-300 whitespace-pre-wrap">
                                                            {me_view.bio.clone()}
                                                        </p>
                                                    }.into_any()
                                                }}
                                                // Stats row — only "Posts" is real; there is no
                                                // follower-count API, so we never fabricate one.
                                                <div class="mt-3 flex items-center justify-center gap-6 sm:justify-start">
                                                    <div class="flex flex-col items-center sm:items-start">
                                                        <span class="text-lg font-bold text-white">
                                                            {move || posts.get().len()}
                                                        </span>
                                                        <span class="text-xs uppercase tracking-wider text-muted">"Posts"</span>
                                                    </div>
                                                </div>
                                            </>
                                        }.into_any()
                                    }}
                                </div>
                            </div>

                            // DID row — truncated mono, Copy (full to clipboard) + QR popup.
                            {
                                let full_did = me_did_row.did_key.clone();
                                let short = truncate_did(&full_did);
                                if full_did.is_empty() {
                                    view! { <></> }.into_any()
                                } else {
                                    view! {
                                        <div class="mt-5 flex flex-wrap items-center justify-center gap-2 border-t border-surface pt-4 sm:justify-start">
                                            <code class="font-mono text-xs text-slate-500 dark:text-slate-400 break-all">
                                                {short}
                                            </code>
                                            <button
                                                type="button"
                                                on:click=on_copy_did.clone()
                                                class="unfrost inline-flex items-center gap-1 rounded-full bg-white/10 hover:bg-white/20 border border-surface text-primary px-3 py-1 text-xs font-medium"
                                            >
                                                {move || if copied.get() { "Copied" } else { "Copy" }}
                                            </button>
                                            <button
                                                type="button"
                                                on:click=move |_| qr_open.set(true)
                                                title="Show QR code"
                                                aria-label="Show QR code"
                                                class="unfrost inline-flex items-center gap-1 rounded-full bg-white/10 hover:bg-white/20 border border-surface text-primary px-2.5 py-1 text-xs font-medium"
                                            >
                                                <svg viewBox="0 0 24 24" class="h-4 w-4" fill="currentColor" aria-hidden="true">
                                                    <path d="M3 3h7v7H3V3zm2 2v3h3V5H5z" />
                                                    <path d="M14 3h7v7h-7V3zm2 2v3h3V5h-3z" />
                                                    <path d="M3 14h7v7H3v-7zm2 2v3h3v-3H5z" />
                                                    <path d="M13 13h3v3h-3v-3zm5 0h3v3h-3v-3zm-5 5h3v3h-3v-3zm5 0h3v3h-3v-3z" />
                                                </svg>
                                            </button>
                                        </div>
                                    }.into_any()
                                }
                            }

                            // Actions row (only shown when not editing).
                            {move || if editing.get() {
                                view! { <></> }.into_any()
                            } else if is_self_view.get() {
                                view! {
                                    <div class="mt-4 flex justify-center sm:justify-start">
                                        <button
                                            type="button"
                                            on:click=move |_| { editing.set(true); }
                                            class="unfrost inline-flex items-center gap-1 rounded-full bg-white/10 hover:bg-white/20 border border-surface text-primary px-4 py-1.5 text-xs font-semibold"
                                        >
                                            "Edit profile"
                                        </button>
                                    </div>
                                }.into_any()
                            } else {
                                let click = on_follow_click.clone();
                                let target_did = me_actions.did_key.clone();
                                let chat_href = format!("/chat/{}", target_did);
                                view! {
                                    <div class="mt-4 flex flex-wrap justify-center gap-2 sm:justify-start">
                                        <button
                                            type="button"
                                            on:click=click
                                            class=move || if following.get() {
                                                "transition-quick unfrost inline-flex items-center gap-1 rounded-full bg-white/10 hover:bg-white/20 border border-surface text-primary px-5 py-1.5 text-xs font-semibold".to_string()
                                            } else {
                                                "transition-quick unfrost inline-flex items-center gap-1 rounded-full bg-accent hover:bg-amber-300 text-accent-text px-5 py-1.5 text-xs font-semibold".to_string()
                                            }
                                        >
                                            {move || if following.get() { "Following" } else { "Follow" }}
                                        </button>
                                        <NavLink
                                            href=chat_href
                                            class="unfrost inline-flex items-center gap-1 rounded-full bg-white/10 hover:bg-white/20 border border-surface text-primary px-5 py-1.5 text-xs font-semibold"
                                        >
                                            "Message"
                                        </NavLink>
                                    </div>
                                }.into_any()
                            }}
                            }.into_any()
                        },
                        None => view! {
                            <p class="text-sm text-slate-500 dark:text-slate-400">"Loading profile…"</p>
                        }.into_any(),
                    }}
                </header>

                // Posts → square photo grid.
                <section>
                    <h2 class="px-1 mb-3 text-xs uppercase tracking-wider text-muted">
                        "Posts"
                    </h2>
                    {move || {
                        let list = posts.get();
                        if list.is_empty() {
                            view! {
                                <p class="frosted-card p-6 text-sm text-muted text-center">
                                    "No posts yet."
                                </p>
                            }.into_any()
                        } else {
                            let self_view = is_self_view.get();
                            view! {
                                <div class="grid grid-cols-3 gap-1 sm:gap-2">
                                    <For
                                        each=move || posts.get()
                                        key=|p| p.id.clone()
                                        children=move |p: Post| {
                                            let post_href = format!("/p/{}", p.id);
                                            let likes = reaction_count(&p);
                                            let comments = p.comments.len();
                                            let post_id = p.id.clone();
                                            let has_image = !p.images.is_empty();
                                            let thumb = if has_image {
                                                ipfs::gateway_url(&p.images[0].cid, None)
                                            } else {
                                                String::new()
                                            };
                                            let caption = p.caption.clone();
                                            view! {
                                                <div class="group relative aspect-square overflow-hidden rounded-md bg-surface">
                                                    <NavLink
                                                        href=post_href
                                                        class="block h-full w-full"
                                                    >
                                                        {if has_image {
                                                            view! {
                                                                <img
                                                                    src=thumb
                                                                    alt=""
                                                                    loading="lazy"
                                                                    class="aspect-square w-full object-cover"
                                                                />
                                                            }.into_any()
                                                        } else {
                                                            // Caption-only tile — subtle bg + clamped text.
                                                            view! {
                                                                <div class="flex h-full w-full items-center justify-center bg-black/5 dark:bg-white/5 p-3">
                                                                    <p class="text-center text-[11px] leading-snug text-slate-600 dark:text-slate-300 line-clamp-4">
                                                                        {caption}
                                                                    </p>
                                                                </div>
                                                            }.into_any()
                                                        }}
                                                        // Hover overlay with like/comment counts.
                                                        <div class="pointer-events-none absolute inset-0 flex items-center justify-center gap-4 bg-black/0 text-white opacity-0 transition group-hover:bg-black/35 group-hover:opacity-100">
                                                            <span class="inline-flex items-center gap-1.5 text-sm font-semibold">
                                                                <HeartIcon class="h-4 w-4" filled=true /> {likes}
                                                            </span>
                                                            <span class="inline-flex items-center gap-1.5 text-sm font-semibold">
                                                                <CommentIcon class="h-4 w-4" /> {comments}
                                                            </span>
                                                        </div>
                                                    </NavLink>
                                                    {if self_view {
                                                        view! {
                                                            <button
                                                                type="button"
                                                                title="Delete post"
                                                                on:click=move |_| {
                                                                    let id = post_id.clone();
                                                                    spawn_local(async move {
                                                                        if delete_post(&id).await.is_ok() {
                                                                            posts.update(|v| v.retain(|x| x.id != id));
                                                                        }
                                                                    });
                                                                }
                                                                class="absolute right-1.5 top-1.5 inline-flex h-7 w-7 items-center justify-center rounded-full bg-black/45 text-white opacity-0 transition hover:bg-rose-600 group-hover:opacity-100"
                                                            >
                                                                <svg viewBox="0 0 24 24" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                                                                    <path d="M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2m2 0v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" />
                                                                </svg>
                                                            </button>
                                                        }.into_any()
                                                    } else {
                                                        view! { <></> }.into_any()
                                                    }}
                                                </div>
                                            }
                                        }
                                    />
                                </div>
                            }.into_any()
                        }
                    }}
                </section>
            </div>

            // DID QR popup — frosted Modal showing the SVG QR of the DID.
            <Modal open=qr_open>
                {move || {
                    let did = profile.get().map(|p| p.did_key).unwrap_or_default();
                    let svg = invite_qr_svg(&did);
                    view! {
                        <div class="frosted-card p-6 space-y-4 text-center">
                            <h3 class="text-base font-semibold text-primary">"My DID"</h3>
                            {if let Some(svg) = svg {
                                view! {
                                    <div
                                        class="mx-auto w-fit rounded-xl bg-white p-3 flex items-center justify-center"
                                        inner_html=svg
                                    ></div>
                                }.into_any()
                            } else {
                                view! {
                                    <p class="text-sm text-muted">"Could not render QR."</p>
                                }.into_any()
                            }}
                            <code class="block font-mono text-[10px] text-slate-500 dark:text-slate-400 break-all">
                                {did}
                            </code>
                            <p class="text-xs text-muted">"Scan to get my DID"</p>
                            <button
                                type="button"
                                on:click=move |_| qr_open.set(false)
                                class="unfrost inline-flex items-center gap-1 rounded-full bg-white/10 hover:bg-white/20 border border-surface text-primary px-5 py-1.5 text-xs font-semibold"
                            >
                                "Close"
                            </button>
                        </div>
                    }
                }}
            </Modal>
        </>
    }
}
