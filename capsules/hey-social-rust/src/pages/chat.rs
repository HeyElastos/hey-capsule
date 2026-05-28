// Chat — Telegram-style DM + group page.
//
// Routes:
//   /chat            ContactList only (mobile) / + EmptyConversation (desktop)
//   /chat/:did       1:1 DM conversation
//   /chat/g/:id      Group conversation
//
// Desktop (md+):  two-pane layout — combined contact list on the left
//                 (groups + DMs), conversation on the right.
// Mobile (< md):  single-pane that swaps based on URL.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::{use_navigate, use_params_map};
use leptos_router::NavigateOptions;
use wasm_bindgen::JsCast;
use web_sys::{HtmlInputElement, KeyboardEvent};

use crate::api::dms::{
    accept_invite, generate_invite, get_expiry_secs, list_contacts, mark_read as mark_dm_read,
    prune_expired, read_conversation, send_message as send_dm, set_expiry_secs, DmContact,
    DmMessage,
};
use crate::api::groups::{
    list_groups, mark_read as mark_group_read, read_group, read_messages,
    send_message as send_group, Group, GroupMessage,
};
use crate::app_modals::AppModals;
use crate::components::icons::{ArrowRightIcon, ChatIcon, PlusIcon, UserIcon};
use crate::components::{FloatingDock, NavLink, TopHeader};

#[component]
pub fn Chat() -> impl IntoView {
    let params = use_params_map();
    let active_did = move || params.read().get("did").map(|s| s.to_string()).unwrap_or_default();
    let active_group_id =
        move || params.read().get("group_id").map(|s| s.to_string()).unwrap_or_default();
    let any_active = move || !active_did().is_empty() || !active_group_id().is_empty();

    let dm_contacts: RwSignal<Vec<DmContact>> = RwSignal::new(Vec::new());
    let groups: RwSignal<Vec<Group>> = RwSignal::new(Vec::new());
    let dm_messages: RwSignal<Vec<DmMessage>> = RwSignal::new(Vec::new());
    let group_messages: RwSignal<Vec<GroupMessage>> = RwSignal::new(Vec::new());
    let composer = RwSignal::new(String::new());
    let busy = RwSignal::new(false);

    // Poll contacts + groups every 2s so new messages from peer_receiver
    // surface without manual refresh.
    Effect::new(move |_| {
        spawn_local(async move {
            loop {
                dm_contacts.set(list_contacts().await);
                groups.set(list_groups().await);
                wait_2s().await;
            }
        });
    });

    // When the URL changes, load the right conversation + mark unread=0.
    Effect::new(move |_| {
        let did = active_did();
        let gid = active_group_id();
        if !gid.is_empty() {
            spawn_local(async move {
                let msgs = read_messages(&gid).await;
                group_messages.set(msgs);
                mark_group_read(&gid).await;
            });
        } else if !did.is_empty() {
            spawn_local(async move {
                prune_expired(&did).await;
                let msgs = read_conversation(&did).await;
                dm_messages.set(msgs);
                mark_dm_read(&did).await;
            });
        } else {
            dm_messages.set(Vec::new());
            group_messages.set(Vec::new());
        }
    });

    // Poll active conversation for incoming messages.
    Effect::new(move |_| {
        spawn_local(async move {
            loop {
                let did = active_did();
                let gid = active_group_id();
                if !gid.is_empty() {
                    let msgs = read_messages(&gid).await;
                    group_messages.set(msgs);
                } else if !did.is_empty() {
                    let msgs = read_conversation(&did).await;
                    dm_messages.set(msgs);
                }
                wait_2s().await;
            }
        });
    });

    view! {
        <>
            <TopHeader />
            <FloatingDock />
            <div class="mx-auto max-w-6xl h-[calc(100vh-3.5rem)] pl-20 sm:pl-24 sm:h-[calc(100vh-4.5rem)] flex">
                <div
                    class="w-full md:w-80 md:border-r md:border-surface md:flex"
                    class:hidden=move || any_active()
                    class:md:flex=move || true
                >
                    <ContactList dm_contacts=dm_contacts groups=groups active_did=Signal::derive(active_did) active_group_id=Signal::derive(active_group_id) />
                </div>

                <div
                    class="flex-1 flex flex-col h-full bg-surface-soft/30"
                    class:hidden=move || !any_active()
                    class:md:flex=move || true
                >
                    {move || {
                        let did = active_did();
                        let gid = active_group_id();
                        if !gid.is_empty() {
                            let group = groups.read().iter().find(|g| g.id == gid).cloned();
                            view! {
                                <GroupConversation
                                    group_id=gid
                                    group=group
                                    messages=group_messages
                                    composer=composer
                                    busy=busy
                                />
                            }.into_any()
                        } else if !did.is_empty() {
                            let contact = dm_contacts.read().iter().find(|c| c.did == did).cloned();
                            view! {
                                <DmConversation
                                    did=did
                                    contact=contact
                                    messages=dm_messages
                                    composer=composer
                                    busy=busy
                                />
                            }.into_any()
                        } else {
                            view! { <EmptyConversation /> }.into_any()
                        }
                    }}
                </div>
            </div>
        </>
    }
}

#[component]
fn ContactList(
    dm_contacts: RwSignal<Vec<DmContact>>,
    groups: RwSignal<Vec<Group>>,
    active_did: Signal<String>,
    active_group_id: Signal<String>,
) -> impl IntoView {
    let modals = use_context::<AppModals>().unwrap_or_default();
    // Invite panel state. Mode:
    //   ""        — panel closed
    //   "gen"     — show generated invite link with copy button
    //   "paste"   — show textarea for pasting someone else's invite
    let invite_mode = RwSignal::new(String::new());
    let invite_label = RwSignal::new(String::new());
    let invite_link = RwSignal::new(String::new());
    let invite_paste = RwSignal::new(String::new());
    let invite_error = RwSignal::new(String::new());
    let invite_busy = RwSignal::new(false);
    let navigate = use_navigate();

    let do_generate = move || {
        if invite_busy.get() {
            return;
        }
        invite_error.set(String::new());
        invite_busy.set(true);
        let label = invite_label.get();
        spawn_local(async move {
            match generate_invite(&label).await {
                Ok(link) => {
                    invite_link.set(link);
                }
                Err(e) => invite_error.set(e),
            }
            invite_busy.set(false);
        });
    };

    let do_accept = {
        let navigate = navigate.clone();
        move || {
            if invite_busy.get() {
                return;
            }
            invite_error.set(String::new());
            invite_busy.set(true);
            let token = invite_paste.get();
            let navigate = navigate.clone();
            spawn_local(async move {
                match accept_invite(&token).await {
                    Ok(did) => {
                        invite_paste.set(String::new());
                        invite_mode.set(String::new());
                        navigate(&format!("/chat/{did}"), NavigateOptions::default());
                    }
                    Err(e) => invite_error.set(e),
                }
                invite_busy.set(false);
            });
        }
    };

    let copy_invite = move |_| {
        let link = invite_link.get();
        if link.is_empty() {
            return;
        }
        if let Some(win) = web_sys::window() {
            let clipboard = win.navigator().clipboard();
            let _ = clipboard.write_text(&link);
        }
    };

    view! {
        <div class="w-full flex flex-col">
            <header class="px-4 py-3 border-b border-surface flex items-center justify-between">
                <h2 class="logo-handwritten text-3xl text-primary">"Chat"</h2>
                <div class="flex items-center gap-1">
                    <button
                        type="button"
                        on:click=move |_| modals.new_group_open.set(true)
                        class="icon-btn-ghost p-2"
                        aria-label="New group"
                        title="New group"
                    >
                        <svg viewBox="0 0 24 24" class="h-4 w-4" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2" />
                            <circle cx="9" cy="7" r="4" />
                            <path d="M23 21v-2a4 4 0 0 0-3-3.87" />
                            <path d="M16 3.13a4 4 0 0 1 0 7.75" />
                        </svg>
                    </button>
                    <button
                        type="button"
                        on:click=move |_| {
                            invite_mode.update(|v| {
                                *v = if v == "gen" { String::new() } else { "gen".into() };
                            });
                            invite_link.set(String::new());
                            invite_error.set(String::new());
                        }
                        class="icon-btn-ghost p-2"
                        aria-label="New invite link"
                        title="Create invite link (metadata-safe)"
                    >
                        <PlusIcon class="h-4 w-4" />
                    </button>
                    <button
                        type="button"
                        on:click=move |_| {
                            invite_mode.update(|v| {
                                *v = if v == "paste" { String::new() } else { "paste".into() };
                            });
                            invite_paste.set(String::new());
                            invite_error.set(String::new());
                        }
                        class="icon-btn-ghost p-2"
                        aria-label="Paste invite link"
                        title="Accept invite from someone"
                    >
                        <svg viewBox="0 0 24 24" class="h-4 w-4" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <rect x="9" y="2" width="6" height="4" rx="1" />
                            <path d="M9 4H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V6a2 2 0 0 0-2-2h-3" />
                        </svg>
                    </button>
                    <span class="inline-flex items-center gap-1 text-[10px] uppercase tracking-wider text-emerald-400" title="Per-pair anonymous queues + sealed-sender envelope. Provider sees only random queue ids and opaque ciphertext; no DIDs in topic names; no plaintext in flight. Hybrid PQ (ML-KEM-768 + X25519 + ChaCha20-Poly1305).">
                        <svg viewBox="0 0 24 24" class="h-3 w-3" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <rect x="3" y="11" width="18" height="11" rx="2" />
                            <path d="M7 11V7a5 5 0 0 1 10 0v4" />
                        </svg>
                        "E2E · PQ · Sealed"
                    </span>
                </div>
            </header>

            {move || match invite_mode.get().as_str() {
                "gen" => {
                    let gen_now = do_generate.clone();
                    view! {
                        <div class="px-4 py-3 border-b border-surface bg-white/5 space-y-2 animate-fade-in">
                            <p class="text-[11px] text-muted leading-snug">
                                "Mint a one-time invite link. Send it via any channel (SMS, email, in person). When they paste it back, you'll appear in each other's contact lists with a metadata-safe queue. No DIDs go on the wire."
                            </p>
                            <input
                                type="text"
                                class="frosted-input text-sm"
                                placeholder="Label (just for you, e.g. \"Bob from work\")"
                                prop:value=move || invite_label.get()
                                on:input=move |ev: web_sys::Event| {
                                    if let Some(t) = ev.target() {
                                        if let Ok(i) = t.dyn_into::<HtmlInputElement>() {
                                            invite_label.set(i.value());
                                        }
                                    }
                                }
                            />
                            <button
                                type="button"
                                on:click=move |_| gen_now()
                                prop:disabled=move || invite_busy.get()
                                class="unfrost w-full rounded-full bg-accent hover:bg-amber-300 disabled:opacity-40 text-accent-text font-semibold px-3 py-1.5 text-xs"
                            >
                                {move || if invite_busy.get() { "Generating…" } else { "Generate invite link" }}
                            </button>
                            {move || {
                                let link = invite_link.get();
                                if link.is_empty() { view! { <></> }.into_any() }
                                else { view! {
                                    <div class="space-y-1">
                                        <textarea
                                            class="frosted-input text-[10px] font-mono w-full h-20 break-all"
                                            readonly=true
                                            prop:value=link.clone()
                                        ></textarea>
                                        <button
                                            type="button"
                                            on:click=copy_invite
                                            class="unfrost w-full rounded-full bg-white/10 hover:bg-white/20 text-primary font-medium px-3 py-1.5 text-xs"
                                        >
                                            "Copy link"
                                        </button>
                                    </div>
                                }.into_any() }
                            }}
                            {move || {
                                let m = invite_error.get();
                                if m.is_empty() { view! { <></> }.into_any() }
                                else { view! { <p class="text-xs text-red-400">{m}</p> }.into_any() }
                            }}
                        </div>
                    }.into_any()
                }
                "paste" => {
                    let accept_now = do_accept.clone();
                    view! {
                        <div class="px-4 py-3 border-b border-surface bg-white/5 space-y-2 animate-fade-in">
                            <p class="text-[11px] text-muted leading-snug">
                                "Paste an invite link someone shared with you. We'll send back a handshake on their queue (encrypted to their pubkeys) and the conversation opens."
                            </p>
                            <textarea
                                class="frosted-input text-[10px] font-mono w-full h-20 break-all"
                                placeholder="hey-invite:…"
                                prop:value=move || invite_paste.get()
                                on:input=move |ev: web_sys::Event| {
                                    if let Some(t) = ev.target() {
                                        if let Ok(i) = t.dyn_into::<web_sys::HtmlTextAreaElement>() {
                                            invite_paste.set(i.value());
                                        }
                                    }
                                }
                            ></textarea>
                            <button
                                type="button"
                                on:click=move |_| accept_now()
                                prop:disabled=move || invite_busy.get() || invite_paste.get().trim().is_empty()
                                class="unfrost w-full rounded-full bg-accent hover:bg-amber-300 disabled:opacity-40 text-accent-text font-semibold px-3 py-1.5 text-xs"
                            >
                                {move || if invite_busy.get() { "Accepting…" } else { "Accept invite" }}
                            </button>
                            {move || {
                                let m = invite_error.get();
                                if m.is_empty() { view! { <></> }.into_any() }
                                else { view! { <p class="text-xs text-red-400">{m}</p> }.into_any() }
                            }}
                        </div>
                    }.into_any()
                }
                _ => view! { <></> }.into_any(),
            }}

            <ul class="flex-1 overflow-y-auto">
                // Groups first.
                <For
                    each=move || groups.get()
                    key=|g| g.id.clone()
                    children=move |g: Group| {
                        let is_active = active_group_id.get() == g.id;
                        let href = format!("/chat/g/{}", g.id);
                        view! {
                            <li>
                                <NavLink
                                    href=href
                                    class=if is_active {
                                        "flex items-center gap-3 px-4 py-3 bg-white/10 border-l-2 border-accent transition-colors"
                                    } else {
                                        "flex items-center gap-3 px-4 py-3 hover:bg-white/5 border-l-2 border-transparent transition-colors"
                                    }
                                >
                                    <GroupAvatar name=g.name.clone() />
                                    <div class="flex-1 min-w-0">
                                        <div class="flex items-baseline justify-between gap-2">
                                            <span class="text-sm font-medium text-primary truncate">
                                                {g.name.clone()}
                                            </span>
                                            <span class="text-[10px] text-muted shrink-0">{ts_short(g.last_ts)}</span>
                                        </div>
                                        <div class="flex items-center justify-between gap-2">
                                            <p class="text-xs text-muted truncate">
                                                <span class="text-amber-400">"#"</span>" "{g.members.len().to_string()}" · "{g.last_preview.clone()}
                                            </p>
                                            {if g.unread > 0 {
                                                view! {
                                                    <span class="inline-flex h-5 min-w-5 items-center justify-center rounded-full bg-accent text-accent-text text-[10px] font-bold px-1.5 shrink-0">
                                                        {if g.unread > 9 { "9+".to_string() } else { g.unread.to_string() }}
                                                    </span>
                                                }.into_any()
                                            } else { view! { <></> }.into_any() }}
                                        </div>
                                    </div>
                                </NavLink>
                            </li>
                        }
                    }
                />
                // Then DMs.
                <For
                    each=move || dm_contacts.get()
                    key=|c| c.did.clone()
                    children=move |c: DmContact| {
                        let is_active = active_did.get() == c.did;
                        let did_for_link = c.did.clone();
                        view! {
                            <li>
                                <NavLink
                                    href=format!("/chat/{}", did_for_link)
                                    class=if is_active {
                                        "flex items-center gap-3 px-4 py-3 bg-white/10 border-l-2 border-accent transition-colors"
                                    } else {
                                        "flex items-center gap-3 px-4 py-3 hover:bg-white/5 border-l-2 border-transparent transition-colors"
                                    }
                                >
                                    <DmAvatar name=c.name.clone() did=c.did.clone() />
                                    <div class="flex-1 min-w-0">
                                        <div class="flex items-baseline justify-between gap-2">
                                            <span class="text-sm font-medium text-primary truncate">
                                                {display_dm_name(&c)}
                                            </span>
                                            <span class="text-[10px] text-muted shrink-0">{ts_short(c.last_ts)}</span>
                                        </div>
                                        <div class="flex items-center justify-between gap-2">
                                            <p class="text-xs text-muted truncate">{c.last_preview.clone()}</p>
                                            {if c.unread > 0 {
                                                view! {
                                                    <span class="inline-flex h-5 min-w-5 items-center justify-center rounded-full bg-accent text-accent-text text-[10px] font-bold px-1.5 shrink-0">
                                                        {if c.unread > 9 { "9+".to_string() } else { c.unread.to_string() }}
                                                    </span>
                                                }.into_any()
                                            } else { view! { <></> }.into_any() }}
                                        </div>
                                    </div>
                                </NavLink>
                            </li>
                        }
                    }
                />

                {move || {
                    let n_g = groups.read().len();
                    let n_d = dm_contacts.read().len();
                    if n_g + n_d == 0 {
                        view! {
                            <div class="px-4 py-12 text-center text-sm text-muted">
                                <p>"No conversations yet."</p>
                                <p class="mt-2 text-xs">"Tap " <strong>"+"</strong> " to start a chat or the 👥 icon to create a group."</p>
                            </div>
                        }.into_any()
                    } else { view! { <></> }.into_any() }
                }}
            </ul>
        </div>
    }
}

#[component]
fn DmConversation(
    did: String,
    contact: Option<DmContact>,
    messages: RwSignal<Vec<DmMessage>>,
    composer: RwSignal<String>,
    busy: RwSignal<bool>,
) -> impl IntoView {
    let navigate = use_navigate();
    let did_for_send = did.clone();
    let display = contact
        .as_ref()
        .map(display_dm_name)
        .unwrap_or_else(|| short_did(&did));

    let expiry_secs = RwSignal::new(0i64);
    {
        let did_for_load = did.clone();
        Effect::new(move |_| {
            let d = did_for_load.clone();
            spawn_local(async move {
                expiry_secs.set(get_expiry_secs(&d).await);
            });
        });
    }
    let on_expiry_change = {
        let did_for_set = did.clone();
        move |ev: web_sys::Event| {
            let Some(t) = ev.target() else { return };
            let Ok(sel) = t.dyn_into::<web_sys::HtmlSelectElement>() else { return };
            let secs: i64 = sel.value().parse().unwrap_or(0);
            let d = did_for_set.clone();
            spawn_local(async move {
                let _ = set_expiry_secs(&d, secs).await;
                expiry_secs.set(secs);
                prune_expired(&d).await;
                let msgs = read_conversation(&d).await;
                messages.set(msgs);
            });
        }
    };

    let send = {
        let did = did_for_send.clone();
        move || {
            if busy.get() {
                return;
            }
            let text = composer.get();
            if text.trim().is_empty() {
                return;
            }
            let did = did.clone();
            busy.set(true);
            spawn_local(async move {
                if let Ok(_m) = send_dm(&did, &text).await {
                    composer.set(String::new());
                    let updated = read_conversation(&did).await;
                    messages.set(updated);
                }
                busy.set(false);
            });
        }
    };

    let did_for_avatar = did.clone();
    let did_for_link = did.clone();
    let back_to_list = {
        let navigate = navigate.clone();
        move |_| navigate("/chat", NavigateOptions::default())
    };

    view! {
        <header class="flex items-center gap-3 px-4 py-3 border-b border-surface bg-surface-soft/80 backdrop-blur">
            <button
                type="button"
                on:click=back_to_list
                class="icon-btn-ghost p-2 md:hidden"
                aria-label="Back"
            >
                <svg viewBox="0 0 24 24" class="h-5 w-5" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                    <path d="m15 18-6-6 6-6" />
                </svg>
            </button>
            <DmAvatar name=display.clone() did=did_for_avatar />
            <div class="flex-1 min-w-0">
                <NavLink href=format!("/profile/{}", did_for_link) class="text-sm font-medium text-primary hover:underline truncate block">
                    {display.clone()}
                </NavLink>
                <p class="text-[10px] font-mono text-muted truncate">{short_did(&did)}</p>
            </div>
            <select
                class="frosted-input !rounded-full !py-1 !px-2 text-[11px] !w-auto hidden sm:inline-block"
                title="Auto-delete messages after this much time has passed."
                on:change=on_expiry_change
            >
                <option value="0" selected=move || expiry_secs.get() == 0>"Keep forever"</option>
                <option value="3600" selected=move || expiry_secs.get() == 3600>"1 hour"</option>
                <option value="86400" selected=move || expiry_secs.get() == 86400>"1 day"</option>
                <option value="604800" selected=move || expiry_secs.get() == 604800>"1 week"</option>
                <option value="2592000" selected=move || expiry_secs.get() == 2592000>"30 days"</option>
            </select>
            <span class="hidden sm:inline-flex items-center gap-1 rounded-full bg-emerald-500/15 border border-emerald-500/30 px-2 py-0.5 text-[10px] text-emerald-300" title="End-to-end encrypted (ML-KEM-768 + X25519).">
                <svg viewBox="0 0 24 24" class="h-3 w-3" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                    <rect x="3" y="11" width="18" height="11" rx="2" />
                    <path d="M7 11V7a5 5 0 0 1 10 0v4" />
                </svg>
                "E2E · PQ"
            </span>
        </header>

        <div class="flex-1 overflow-y-auto px-3 py-4 space-y-2">
            {move || {
                let list = messages.get();
                if list.is_empty() {
                    view! {
                        <div class="flex h-full items-center justify-center text-sm text-muted">
                            <p>"Say hi 👋"</p>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <For
                            each=move || messages.get()
                            key=|m| m.id.clone()
                            children=move |m: DmMessage| view! { <DmBubble m=m /> }
                        />
                    }.into_any()
                }
            }}
        </div>

        <Composer composer=composer busy=busy send=send.clone() />
    }
}

#[component]
fn GroupConversation(
    group_id: String,
    group: Option<Group>,
    messages: RwSignal<Vec<GroupMessage>>,
    composer: RwSignal<String>,
    busy: RwSignal<bool>,
) -> impl IntoView {
    let navigate = use_navigate();
    let title = group
        .as_ref()
        .map(|g| g.name.clone())
        .unwrap_or_else(|| "Group".into());
    let member_count = group.as_ref().map(|g| g.members.len()).unwrap_or(0);
    let group_id_for_send = group_id.clone();

    let send = {
        let group_id = group_id_for_send.clone();
        move || {
            if busy.get() {
                return;
            }
            let text = composer.get();
            if text.trim().is_empty() {
                return;
            }
            let group_id = group_id.clone();
            busy.set(true);
            spawn_local(async move {
                if let Ok(_m) = send_group(&group_id, &text).await {
                    composer.set(String::new());
                    let updated = read_messages(&group_id).await;
                    messages.set(updated);
                }
                busy.set(false);
            });
        }
    };

    let back_to_list = {
        let navigate = navigate.clone();
        move |_| navigate("/chat", NavigateOptions::default())
    };

    let group_for_chips = group.clone();
    let show_members = RwSignal::new(false);

    view! {
        <header class="flex items-center gap-3 px-4 py-3 border-b border-surface bg-surface-soft/80 backdrop-blur">
            <button
                type="button"
                on:click=back_to_list
                class="icon-btn-ghost p-2 md:hidden"
                aria-label="Back"
            >
                <svg viewBox="0 0 24 24" class="h-5 w-5" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                    <path d="m15 18-6-6 6-6" />
                </svg>
            </button>
            <GroupAvatar name=title.clone() />
            <button
                type="button"
                on:click=move |_| show_members.update(|v| *v = !*v)
                class="flex-1 min-w-0 text-left"
            >
                <p class="text-sm font-medium text-primary truncate">{title.clone()}</p>
                <p class="text-[10px] text-muted">{format!("{member_count} member{}", if member_count == 1 { "" } else { "s" })}</p>
            </button>
            <span class="hidden sm:inline-flex items-center gap-1 rounded-full bg-emerald-500/15 border border-emerald-500/30 px-2 py-0.5 text-[10px] text-emerald-300" title="Per-recipient ML-KEM-768 + X25519 encryption. Each member sees their own envelope.">
                <svg viewBox="0 0 24 24" class="h-3 w-3" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                    <rect x="3" y="11" width="18" height="11" rx="2" />
                    <path d="M7 11V7a5 5 0 0 1 10 0v4" />
                </svg>
                "E2E · PQ"
            </span>
        </header>

        {move || if show_members.get() {
            let members = group_for_chips.as_ref().map(|g| g.members.clone()).unwrap_or_default();
            view! {
                <div class="border-b border-surface bg-white/5 px-4 py-2 flex flex-wrap gap-1.5 animate-fade-in">
                    {members.into_iter().map(|did| view! {
                        <NavLink
                            href=format!("/profile/{}", did)
                            class="inline-flex items-center gap-1 rounded-full bg-white/10 border border-surface px-2.5 py-0.5 text-[10px] font-mono text-muted hover:text-primary hover:bg-white/20"
                        >
                            <UserIcon class="h-3 w-3" />
                            {short_did(&did)}
                        </NavLink>
                    }).collect::<Vec<_>>()}
                </div>
            }.into_any()
        } else { view! { <></> }.into_any() }}

        <div class="flex-1 overflow-y-auto px-3 py-4 space-y-2">
            {move || {
                let list = messages.get();
                if list.is_empty() {
                    view! {
                        <div class="flex h-full items-center justify-center text-sm text-muted">
                            <p>"No messages yet. Start the conversation 👋"</p>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <For
                            each=move || messages.get()
                            key=|m| m.id.clone()
                            children=move |m: GroupMessage| view! { <GroupBubble m=m /> }
                        />
                    }.into_any()
                }
            }}
        </div>

        <Composer composer=composer busy=busy send=send.clone() />
    }
}

#[component]
fn Composer(
    composer: RwSignal<String>,
    busy: RwSignal<bool>,
    send: impl Fn() + 'static + Clone + Send + Sync,
) -> impl IntoView {
    let on_input = move |ev: web_sys::Event| {
        if let Some(t) = ev.target() {
            if let Ok(i) = t.dyn_into::<HtmlInputElement>() {
                composer.set(i.value());
            }
        }
    };
    view! {
        <div class="px-3 py-3 border-t border-surface bg-surface-soft/80 backdrop-blur">
            <div class="flex items-end gap-2">
                <input
                    type="text"
                    class="frosted-input text-sm flex-1"
                    placeholder="Type a message…"
                    maxlength="4096"
                    prop:value=move || composer.get()
                    on:input=on_input
                    on:keydown={
                        let send = send.clone();
                        move |ev: KeyboardEvent| {
                            if ev.key() == "Enter" && !ev.shift_key() {
                                ev.prevent_default();
                                send();
                            }
                        }
                    }
                />
                <button
                    type="button"
                    on:click={
                        let send = send.clone();
                        move |_| send()
                    }
                    prop:disabled=move || busy.get() || composer.get().trim().is_empty()
                    class="unfrost rounded-full bg-accent hover:bg-amber-300 disabled:opacity-40 disabled:cursor-not-allowed text-accent-text font-semibold p-3"
                    aria-label="Send"
                >
                    <ArrowRightIcon class="h-4 w-4" />
                </button>
            </div>
        </div>
    }
}

#[component]
fn DmBubble(m: DmMessage) -> impl IntoView {
    let row_class = if m.mine { "flex justify-end" } else { "flex justify-start" };
    let bubble_class = if m.mine {
        "max-w-[78%] rounded-2xl rounded-br-md bg-accent text-accent-text px-3 py-2 shadow-sm"
    } else {
        "max-w-[78%] rounded-2xl rounded-bl-md bg-white/10 border border-surface text-primary px-3 py-2 shadow-sm"
    };
    let ts_class = if m.mine {
        "text-[10px] text-accent-text/70 mt-0.5 text-right"
    } else {
        "text-[10px] text-muted mt-0.5"
    };
    let ts_text = ts_short(m.ts);
    let enc_label = if m.encrypted { "·🔒" } else { "·!" };
    let enc_title = if m.encrypted {
        "Encrypted (ML-KEM-768 + X25519)"
    } else {
        "Plaintext bootstrap (no peer pubkeys yet)"
    };
    view! {
        <div class=row_class>
            <div class=bubble_class>
                <p class="text-sm whitespace-pre-wrap break-words">{m.text}</p>
                <p class=ts_class>
                    <span>{ts_text}</span>
                    <span class="ml-1 text-[9px] opacity-70" title=enc_title>{enc_label}</span>
                </p>
            </div>
        </div>
    }
}

#[component]
fn GroupBubble(m: GroupMessage) -> impl IntoView {
    let row_class = if m.mine { "flex justify-end" } else { "flex justify-start" };
    let bubble_class = if m.mine {
        "max-w-[78%] rounded-2xl rounded-br-md bg-accent text-accent-text px-3 py-2 shadow-sm"
    } else {
        "max-w-[78%] rounded-2xl rounded-bl-md bg-white/10 border border-surface text-primary px-3 py-2 shadow-sm"
    };
    let ts_class = if m.mine {
        "text-[10px] text-accent-text/70 mt-0.5 text-right"
    } else {
        "text-[10px] text-muted mt-0.5"
    };
    let ts_text = ts_short(m.ts);
    let enc_label = if m.encrypted { "·🔒" } else { "·!" };
    let enc_title = if m.encrypted {
        "Encrypted (ML-KEM-768 + X25519)"
    } else {
        "Plaintext bootstrap (no peer pubkeys yet)"
    };
    let sender_name = if m.sender_name.is_empty() {
        short_did(&m.sender_did)
    } else {
        m.sender_name.clone()
    };
    view! {
        <div class=row_class>
            <div class=bubble_class>
                {if !m.mine {
                    view! {
                        <p class="text-[11px] font-semibold text-accent mb-0.5">{sender_name}</p>
                    }.into_any()
                } else { view! { <></> }.into_any() }}
                <p class="text-sm whitespace-pre-wrap break-words">{m.text}</p>
                <p class=ts_class>
                    <span>{ts_text}</span>
                    <span class="ml-1 text-[9px] opacity-70" title=enc_title>{enc_label}</span>
                </p>
            </div>
        </div>
    }
}

#[component]
fn DmAvatar(name: String, did: String) -> impl IntoView {
    let letters = if !name.is_empty() {
        initial_letters(&name)
    } else {
        short_did(&did).chars().take(2).collect::<String>().to_uppercase()
    };
    view! {
        <div class="flex h-10 w-10 flex-none items-center justify-center rounded-full bg-gradient-to-br from-accent to-amber-600 text-accent-text text-sm font-bold shadow-sm">
            {letters}
        </div>
    }
}

#[component]
fn GroupAvatar(name: String) -> impl IntoView {
    let letters = initial_letters(&name);
    view! {
        <div class="flex h-10 w-10 flex-none items-center justify-center rounded-full bg-gradient-to-br from-emerald-400 to-cyan-600 text-white text-sm font-bold shadow-sm">
            {letters}
        </div>
    }
}

#[component]
fn EmptyConversation() -> impl IntoView {
    view! {
        <div class="flex h-full items-center justify-center px-6 text-center">
            <div>
                <div class="inline-flex h-16 w-16 items-center justify-center rounded-2xl border border-white/20 bg-white/10 backdrop-blur-xl text-accent">
                    <ChatIcon class="h-7 w-7" />
                </div>
                <h3 class="mt-4 logo-handwritten text-3xl text-primary">"Pick a conversation"</h3>
                <p class="mt-2 text-sm text-muted max-w-xs mx-auto">
                    "Choose someone on the left, or tap " <strong>"+"</strong> " to start a chat / 👥 to create a group."
                </p>
            </div>
        </div>
    }
}

fn display_dm_name(c: &DmContact) -> String {
    if c.name.is_empty() {
        short_did(&c.did)
    } else {
        c.name.clone()
    }
}

fn short_did(did: &str) -> String {
    let s = did.strip_prefix("did:key:z").unwrap_or(did);
    if s.len() > 12 {
        format!("{}…", s.chars().take(12).collect::<String>())
    } else {
        s.into()
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

fn ts_short(ts: i64) -> String {
    if ts == 0 {
        return String::new();
    }
    let now = js_sys::Date::now();
    let diff_secs = ((now - ts as f64) / 1000.0).max(0.0) as i64;
    if diff_secs < 60 {
        return "now".into();
    }
    let mins = diff_secs / 60;
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
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ts as f64));
    d.to_locale_date_string("en-US", &wasm_bindgen::JsValue::UNDEFINED)
        .as_string()
        .unwrap_or_default()
}

async fn wait_2s() {
    let win = web_sys::window().unwrap();
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let _ = win
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 2_000);
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}
