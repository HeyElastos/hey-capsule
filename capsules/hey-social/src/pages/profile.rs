// Profile — view + edit user profile. Rust port of capsules/hey-social/
// client/src/pages/Profile.jsx (664 lines of React; this is a focused
// subset: identity card, edit name/bio, list of own posts).
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

use crate::api::posts::{get_user_posts, Post};
use crate::api::profile::{
    ensure_profile, follow_user, is_following, unfollow_user, update_profile, upload_avatar,
    Profile as ProfileRecord, ProfileUpdate,
};
use crate::components::{FloatingDock, PostCard, TopHeader};
use crate::session;

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

    view! {
        <>
            <TopHeader />
            <FloatingDock />
            <div class="mx-auto max-w-2xl space-y-6 pl-24 pr-3 py-6 sm:pl-28 sm:pr-6 sm:py-10">
                <header class="frosted-card p-6 animate-fade-up">
                    {move || match profile.get() {
                        Some(me) => view! {
                            <div class="flex items-start gap-4">
                                <label class="relative h-16 w-16 flex-none cursor-pointer">
                                    {if me.avatar.is_empty() {
                                        view! {
                                            <div class="h-16 w-16 rounded-full bg-gradient-to-br from-accent to-amber-600 grid place-items-center text-accent-text text-xl font-bold shadow-sm">
                                                {me.name.chars().next().map(|c| c.to_uppercase().next().unwrap_or(c).to_string()).unwrap_or_else(|| "?".into())}
                                            </div>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <img src=me.avatar.clone() alt="" class="h-16 w-16 rounded-full object-cover ring-1 ring-white/15 shadow-sm" />
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
                                            <span class="absolute -bottom-1 -right-1 inline-flex h-6 w-6 items-center justify-center rounded-full bg-accent text-accent-text text-xs shadow-md">
                                                {move || if avatar_busy.get() { "…" } else { "+" }}
                                            </span>
                                        }.into_any()
                                    } else { view! { <></> }.into_any() }}
                                </label>
                                <div class="min-w-0 flex-1">
                                    {move || if editing.get() && is_self_view.get() {
                                        view! {
                                            <div class="space-y-2">
                                                <input
                                                    class="w-full rounded-lg bg-slate-100 dark:bg-slate-800 border border-slate-200 dark:border-slate-700 px-3 py-2 text-sm"
                                                    type="text"
                                                    maxlength="30"
                                                    prop:value=move || edit_name.get()
                                                    on:input=on_name_input
                                                />
                                                <textarea
                                                    class="w-full rounded-lg bg-slate-100 dark:bg-slate-800 border border-slate-200 dark:border-slate-700 px-3 py-2 text-sm"
                                                    rows="2"
                                                    maxlength="280"
                                                    placeholder="Bio"
                                                    prop:value=move || edit_bio.get()
                                                    on:input=on_bio_input
                                                />
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
                                                        class="rounded-full bg-amber-500 hover:bg-amber-600 disabled:bg-slate-300 dark:disabled:bg-slate-700 text-white font-semibold px-4 py-2 text-xs"
                                                    >
                                                        {move || if saving.get() { "Saving…" } else { "Save" }}
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
                                        }.into_any()
                                    } else {
                                        let me_view = me.clone();
                                        view! {
                                            <>
                                                <h1 class="text-xl font-bold text-slate-900 dark:text-slate-50 truncate">
                                                    {me_view.name.clone()}
                                                </h1>
                                                <p class="mt-1 text-[11px] font-mono text-slate-500 dark:text-slate-400 break-all">
                                                    {me_view.did_key.clone()}
                                                </p>
                                                {if me_view.bio.is_empty() {
                                                    view! { <></> }.into_any()
                                                } else {
                                                    view! {
                                                        <p class="mt-2 text-sm text-slate-700 dark:text-slate-300 whitespace-pre-wrap">
                                                            {me_view.bio.clone()}
                                                        </p>
                                                    }.into_any()
                                                }}
                                                {move || if is_self_view.get() {
                                                    view! {
                                                        <button
                                                            type="button"
                                                            on:click=move |_| { editing.set(true); }
                                                            class="mt-3 inline-flex items-center gap-1 rounded-full bg-white/10 hover:bg-white/20 border border-surface text-primary px-3 py-1.5 text-xs font-medium"
                                                        >
                                                            "Edit profile"
                                                        </button>
                                                    }.into_any()
                                                } else {
                                                    let click = on_follow_click.clone();
                                                    let target_did = params.read().get("did").map(|s| s.to_string()).unwrap_or_default();
                                                    let chat_href = format!("/chat/{}", target_did);
                                                    view! {
                                                        <div class="mt-3 flex flex-wrap gap-2">
                                                            <button
                                                                type="button"
                                                                on:click=click
                                                                class=move || if following.get() {
                                                                    "unfrost inline-flex items-center gap-1 rounded-full bg-white/10 hover:bg-white/20 border border-surface text-primary px-4 py-1.5 text-xs font-semibold".to_string()
                                                                } else {
                                                                    "unfrost inline-flex items-center gap-1 rounded-full bg-accent hover:bg-amber-300 text-accent-text px-4 py-1.5 text-xs font-semibold".to_string()
                                                                }
                                                            >
                                                                {move || if following.get() { "Following" } else { "Follow" }}
                                                            </button>
                                                            <crate::components::NavLink
                                                                href=chat_href
                                                                class="unfrost inline-flex items-center gap-1 rounded-full bg-white/10 hover:bg-white/20 border border-surface text-primary px-4 py-1.5 text-xs font-semibold"
                                                            >
                                                                "Message"
                                                            </crate::components::NavLink>
                                                        </div>
                                                    }.into_any()
                                                }}
                                            </>
                                        }.into_any()
                                    }}
                                </div>
                            </div>
                        }.into_any(),
                        None => view! {
                            <p class="text-sm text-slate-500 dark:text-slate-400">"Loading profile…"</p>
                        }.into_any(),
                    }}
                </header>

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
                            view! {
                                <div class="space-y-4">
                                    <For
                                        each=move || posts.get()
                                        key=|p| p.id.clone()
                                        children=move |p: Post| view! {
                                            <div class="animate-fade-up">
                                                <PostCard post=p />
                                            </div>
                                        }
                                    />
                                </div>
                            }.into_any()
                        }
                    }}
                </section>
            </div>
        </>
    }
}
