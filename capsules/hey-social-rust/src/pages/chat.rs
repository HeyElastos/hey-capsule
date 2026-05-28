// Chat — Telegram-style DM page.
//
// Desktop (md+):  two-pane layout — contact list on the left (320 px),
//                 conversation on the right (flex-1). Both visible at
//                 the same time; selecting a contact switches the
//                 right-pane content.
// Mobile (< md):  single-pane that swaps based on the URL — /chat shows
//                 the contact list, /chat/:did shows the conversation
//                 with a back-arrow header.
//
// Honest about crypto: messages are Ed25519-signed but transmitted as
// plaintext over Carrier. The ML-KEM-768 + X25519 hybrid that
// hey-messenger uses isn't ported yet. A small "Signed, E2E coming"
// badge in the header surfaces this — see components/encryption_badge.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::{use_navigate, use_params_map};
use leptos_router::NavigateOptions;
use wasm_bindgen::JsCast;
use web_sys::{HtmlInputElement, KeyboardEvent};

use crate::api::dms::{
    list_contacts, mark_read, read_conversation, send_message, DmContact, DmMessage,
};
use crate::components::icons::{ArrowRightIcon, ChatIcon};
use crate::components::{FloatingDock, NavLink, TopHeader};

#[component]
pub fn Chat() -> impl IntoView {
    let params = use_params_map();
    let active_did = move || params.read().get("did").map(|s| s.to_string()).unwrap_or_default();

    let contacts: RwSignal<Vec<DmContact>> = RwSignal::new(Vec::new());
    let messages: RwSignal<Vec<DmMessage>> = RwSignal::new(Vec::new());
    let composer = RwSignal::new(String::new());
    let busy = RwSignal::new(false);

    // Poll contacts (so unread badges + last-message updates from
    // peer_receiver land in the UI without a manual refresh).
    Effect::new(move |_| {
        spawn_local(async move {
            loop {
                let list = list_contacts().await;
                contacts.set(list);
                wait_2s().await;
            }
        });
    });

    // When the URL :did changes, load that conversation + mark unread=0.
    Effect::new(move |_| {
        let did = active_did();
        if did.is_empty() {
            messages.set(Vec::new());
            return;
        }
        spawn_local(async move {
            let msgs = read_conversation(&did).await;
            messages.set(msgs);
            mark_read(&did).await;
        });
    });

    // Also poll the active conversation for incoming messages.
    Effect::new(move |_| {
        spawn_local(async move {
            loop {
                let did = active_did();
                if !did.is_empty() {
                    let msgs = read_conversation(&did).await;
                    messages.set(msgs);
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
                // Contact list — full-width on mobile when no :did is selected,
                // 320px fixed sidebar on desktop.
                <div
                    class="w-full md:w-80 md:border-r md:border-surface md:flex"
                    class:hidden=move || !active_did().is_empty()
                    class:md:flex=move || true
                >
                    <ContactList contacts=contacts active_did=Signal::derive(active_did) />
                </div>

                // Conversation pane — hidden on mobile when no :did, full-width
                // on mobile when a :did is selected, flex-1 on desktop.
                <div
                    class="flex-1 flex flex-col h-full bg-surface-soft/30"
                    class:hidden=move || active_did().is_empty()
                    class:md:flex=move || true
                >
                    {move || {
                        let did = active_did();
                        if did.is_empty() {
                            view! { <EmptyConversation /> }.into_any()
                        } else {
                            let contact = contacts.read().iter().find(|c| c.did == did).cloned();
                            view! {
                                <Conversation
                                    did=did
                                    contact=contact
                                    messages=messages
                                    composer=composer
                                    busy=busy
                                />
                            }.into_any()
                        }
                    }}
                </div>
            </div>
        </>
    }
}

#[component]
fn ContactList(contacts: RwSignal<Vec<DmContact>>, active_did: Signal<String>) -> impl IntoView {
    view! {
        <div class="w-full flex flex-col">
            <header class="px-4 py-3 border-b border-surface flex items-baseline justify-between">
                <h2 class="logo-handwritten text-3xl text-primary">"Chat"</h2>
                <span class="inline-flex items-center gap-1 text-[10px] uppercase tracking-wider text-emerald-400" title="End-to-end encrypted with ML-KEM-768 + X25519 hybrid post-quantum + ChaCha20-Poly1305. First message in a thread is plaintext until both sides exchange pubkeys.">
                    <svg viewBox="0 0 24 24" class="h-3 w-3" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <rect x="3" y="11" width="18" height="11" rx="2" />
                        <path d="M7 11V7a5 5 0 0 1 10 0v4" />
                    </svg>
                    "E2E · PQ-hybrid"
                </span>
            </header>
            {move || {
                let list = contacts.get();
                if list.is_empty() {
                    view! {
                        <div class="px-4 py-8 text-center text-sm text-muted">
                            <p>"No conversations yet."</p>
                            <p class="mt-2 text-xs">"Open someone's profile and start a chat."</p>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <ul class="flex-1 overflow-y-auto">
                            <For
                                each=move || contacts.get()
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
                                                <Avatar name=c.name.clone() did=c.did.clone() />
                                                <div class="flex-1 min-w-0">
                                                    <div class="flex items-baseline justify-between gap-2">
                                                        <span class="text-sm font-medium text-primary truncate">
                                                            {display_name(&c)}
                                                        </span>
                                                        <span class="text-[10px] text-muted shrink-0">
                                                            {ts_short(c.last_ts)}
                                                        </span>
                                                    </div>
                                                    <div class="flex items-center justify-between gap-2">
                                                        <p class="text-xs text-muted truncate">
                                                            {c.last_preview.clone()}
                                                        </p>
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
                        </ul>
                    }.into_any()
                }
            }}
        </div>
    }
}

#[component]
fn Conversation(
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
        .map(display_name)
        .unwrap_or_else(|| short_did(&did));

    let on_input = move |ev: web_sys::Event| {
        if let Some(t) = ev.target() {
            if let Ok(i) = t.dyn_into::<HtmlInputElement>() {
                composer.set(i.value());
            }
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
                if let Ok(_m) = send_message(&did, &text).await {
                    composer.set(String::new());
                    let updated = read_conversation(&did).await;
                    messages.set(updated);
                }
                busy.set(false);
            });
        }
    };

    let send_click = {
        let send = send.clone();
        move |_| send()
    };
    let on_keydown = {
        let send = send.clone();
        move |ev: KeyboardEvent| {
            if ev.key() == "Enter" && !ev.shift_key() {
                ev.prevent_default();
                send();
            }
        }
    };

    let back_to_list = move |_| {
        navigate("/chat", NavigateOptions::default());
    };

    let did_for_avatar = did.clone();
    let did_for_link = did.clone();

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
            <Avatar name=display.clone() did=did_for_avatar />
            <div class="flex-1 min-w-0">
                <NavLink href=format!("/profile/{}", did_for_link) class="text-sm font-medium text-primary hover:underline truncate block">
                    {display.clone()}
                </NavLink>
                <p class="text-[10px] font-mono text-muted truncate">{short_did(&did)}</p>
            </div>
            <span class="hidden sm:inline-flex items-center gap-1 rounded-full bg-emerald-500/15 border border-emerald-500/30 px-2 py-0.5 text-[10px] text-emerald-300" title="End-to-end encrypted with ML-KEM-768 (FIPS 203 post-quantum) + X25519 + ChaCha20-Poly1305. First message in a thread is plaintext until both sides exchange pubkeys; subsequent messages are encrypted.">
                <svg viewBox="0 0 24 24" class="h-3 w-3" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                    <rect x="3" y="11" width="18" height="11" rx="2" />
                    <path d="M7 11V7a5 5 0 0 1 10 0v4" />
                </svg>
                "E2E · PQ-hybrid"
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
                            children=move |m: DmMessage| view! { <Bubble m=m /> }
                        />
                    }.into_any()
                }
            }}
        </div>

        <div class="px-3 py-3 border-t border-surface bg-surface-soft/80 backdrop-blur">
            <div class="flex items-end gap-2">
                <input
                    type="text"
                    class="frosted-input text-sm flex-1"
                    placeholder="Type a message…"
                    maxlength="4096"
                    prop:value=move || composer.get()
                    on:input=on_input
                    on:keydown=on_keydown
                />
                <button
                    type="button"
                    on:click=send_click
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
fn Bubble(m: DmMessage) -> impl IntoView {
    let row_class = if m.mine {
        "flex justify-end"
    } else {
        "flex justify-start"
    };
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
fn Avatar(name: String, did: String) -> impl IntoView {
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
fn EmptyConversation() -> impl IntoView {
    view! {
        <div class="flex h-full items-center justify-center px-6 text-center">
            <div>
                <div class="inline-flex h-16 w-16 items-center justify-center rounded-2xl border border-white/20 bg-white/10 backdrop-blur-xl text-accent">
                    <ChatIcon class="h-7 w-7" />
                </div>
                <h3 class="mt-4 logo-handwritten text-3xl text-primary">"Pick a conversation"</h3>
                <p class="mt-2 text-sm text-muted max-w-xs mx-auto">
                    "Choose someone on the left to keep chatting, or open a profile and tap message to start a new thread."
                </p>
            </div>
        </div>
    }
}

fn display_name(c: &DmContact) -> String {
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
