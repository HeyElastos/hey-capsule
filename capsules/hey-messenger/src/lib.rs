use std::borrow::Cow;

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::hooks::{use_navigate, use_params_map};
use leptos_router::path;
use leptos_router::NavigateOptions;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Event, HtmlInputElement, HtmlTextAreaElement, KeyboardEvent, MouseEvent};

use hey_chat::api::dms::{
    accept_invite, generate_invite, list_contacts, mark_read, read_conversation, send_message,
    DmContact, DmMessage,
};
use hey_chat::passkey::{passkey_supported, sign_in_via_runtime};
use hey_chat::session;

// Derive the router base from the iframe mount path. Under YunoHost the
// capsule loads at e.g. `/apps/hey-messenger/` — without this the Router
// sees the full pathname and matches nothing. Same heuristic as hey-social.
fn router_base() -> Cow<'static, str> {
    (|| -> Option<String> {
        let win = web_sys::window()?;
        let path = win.location().pathname().ok()?;
        let idx = path.find("/apps/")?;
        let after = &path[idx + 6..];
        let end = after.find('/').map(|j| idx + 6 + j).unwrap_or(path.len());
        Some(path[..end].to_string())
    })()
    .map(Cow::Owned)
    .unwrap_or(Cow::Borrowed(""))
}

#[component]
pub fn App() -> impl IntoView {
    // Boot against the shared engine (ctx::init already ran in main):
    //   1. redeem any ?home_token=… into an app-scoped session,
    //   2. scrub the token from the visible URL,
    //   3. pre-warm the capability tokens this capsule declared,
    //   4. start the chat receive loop (no-op while signed out).
    spawn_local(async {
        let _ = hey_chat::runtime::redeem_launch_token().await;
        hey_chat::runtime::scrub_launch_token_from_url();
        hey_chat::runtime::acquire_boot_capabilities().await;
    });
    spawn_local(async {
        hey_chat::peer_receiver::run().await;
    });

    let base = router_base();
    view! {
        <Router base=base>
            <Routes fallback=|| view! { <p>"Not found"</p> }>
                <Route path=path!("/") view=Root />
                <Route path=path!("/chat/:did") view=Root />
            </Routes>
        </Router>
    }
}

/// Root view: the passkey sign-in gate wraps the Telegram-desktop shell.
#[component]
fn Root() -> impl IntoView {
    view! { <SignInGate /> }
}

// ── SignInGate ───────────────────────────────────────────────────────────
//
// If there's no session, show a centered sign-in card (passkey first,
// recovery-key fallback). On success, flip `signed_in` to re-render into
// the Shell. We seed the signal from `session::current()` so a returning
// user with a persisted session lands straight in the app.
#[component]
fn SignInGate() -> impl IntoView {
    let signed_in = RwSignal::new(session::current().is_some());

    view! {
        <Show
            when=move || signed_in.get()
            fallback=move || view! { <SignInCard signed_in=signed_in /> }
        >
            <Shell />
        </Show>
    }
}

#[component]
fn SignInCard(signed_in: RwSignal<bool>) -> impl IntoView {
    let busy = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    // Recovery path: when toggled, reveal a nickname input + a button that
    // calls sign_in_via_runtime(Some(name)).
    let recovery_open = RwSignal::new(false);
    let nickname = RwSignal::new(String::new());
    let can_use_passkey = passkey_supported();

    // Shared sign-in driver — `nick = None` is the plain passkey path,
    // `Some(name)` the recovery-key path.
    let do_sign_in = move |nick: Option<String>| {
        if busy.get() {
            return;
        }
        error.set(String::new());
        busy.set(true);
        spawn_local(async move {
            match sign_in_via_runtime(nick).await {
                Ok(s) => {
                    // Engine persists the session on Ok; set it defensively
                    // so `session::current()` is consistent, then re-render.
                    session::set(&s);
                    busy.set(false);
                    signed_in.set(true);
                }
                Err(msg) => {
                    busy.set(false);
                    if msg.contains("NotAllowedError")
                        || msg.contains("AbortError")
                        || msg.to_lowercase().contains("cancel")
                    {
                        error.set("Passkey prompt closed. Tap to try again.".into());
                    } else {
                        error.set(msg);
                    }
                }
            }
        });
    };

    let on_passkey = {
        let do_sign_in = do_sign_in.clone();
        move |_| do_sign_in(None)
    };
    let on_recovery = {
        let do_sign_in = do_sign_in.clone();
        move || {
            let name = nickname.get().trim().to_string();
            if name.is_empty() {
                return;
            }
            do_sign_in(Some(name));
        }
    };

    view! {
        <div class="msgr-signin">
            <div class="msgr-signin-card">
                <div class="msgr-signin-logo">"💬"</div>
                <h1 class="msgr-signin-title">"Hey Chat"</h1>
                <p class="msgr-signin-sub">
                    "Private, peer-to-peer messaging on Elastos. Sign in with the same passkey you set up on this device."
                </p>

                {move || if can_use_passkey {
                    let on_passkey = on_passkey.clone();
                    view! {
                        <button
                            type="button"
                            class="msgr-btn-primary msgr-signin-btn"
                            on:click=on_passkey
                            prop:disabled=move || busy.get()
                        >
                            {move || if busy.get() { "Waiting for passkey…" } else { "Sign in with passkey" }}
                        </button>
                    }.into_any()
                } else {
                    view! {
                        <div class="msgr-signin-warn">
                            "This browser doesn't support passkeys. Use a modern Chrome / Edge / Safari / Firefox."
                        </div>
                    }.into_any()
                }}

                {move || {
                    let msg = error.get();
                    if msg.is_empty() {
                        ().into_any()
                    } else {
                        view! { <p class="msgr-error">{msg}</p> }.into_any()
                    }
                }}

                <button
                    type="button"
                    class="msgr-link-btn"
                    on:click=move |_| recovery_open.update(|v| *v = !*v)
                >
                    "Use a recovery key"
                </button>

                {move || if recovery_open.get() {
                    let on_recovery = on_recovery.clone();
                    let on_recovery_key = on_recovery.clone();
                    view! {
                        <div class="msgr-recovery">
                            <input
                                type="text"
                                class="msgr-input"
                                placeholder="Nickname"
                                prop:value=move || nickname.get()
                                on:input=move |ev: Event| {
                                    if let Some(t) = ev.target() {
                                        if let Ok(i) = t.dyn_into::<HtmlInputElement>() {
                                            nickname.set(i.value());
                                        }
                                    }
                                }
                                on:keydown=move |ev: KeyboardEvent| {
                                    if ev.key() == "Enter" {
                                        ev.prevent_default();
                                        on_recovery_key();
                                    }
                                }
                            />
                            <button
                                type="button"
                                class="msgr-btn-secondary"
                                on:click=move |_| on_recovery()
                                prop:disabled=move || busy.get() || nickname.get().trim().is_empty()
                            >
                                "Continue"
                            </button>
                        </div>
                    }.into_any()
                } else {
                    ().into_any()
                }}
            </div>
        </div>
    }
}

// ── Shell: 2-pane Telegram-desktop layout ────────────────────────────────
#[component]
fn Shell() -> impl IntoView {
    let params = use_params_map();
    let active_did =
        move || params.read().get("did").map(|s| s.to_string()).unwrap_or_default();

    view! {
        <div class="msgr-shell">
            <aside class="msgr-sidebar">
                <ChatList active_did=Signal::derive(active_did) />
            </aside>
            <section class="msgr-main">
                {move || {
                    let did = active_did();
                    if did.is_empty() {
                        view! { <EmptyState /> }.into_any()
                    } else {
                        view! { <Conversation did=did /> }.into_any()
                    }
                }}
            </section>
        </div>
    }
}

#[component]
fn EmptyState() -> impl IntoView {
    view! {
        <div class="msgr-empty">
            <div class="msgr-empty-icon">"💬"</div>
            <h2 class="msgr-empty-title">"Select a chat"</h2>
            <p class="msgr-empty-sub">"Pick a conversation on the left, or add a contact to start."</p>
        </div>
    }
}

// ── ChatList ─────────────────────────────────────────────────────────────
#[component]
fn ChatList(active_did: Signal<String>) -> impl IntoView {
    let contacts: RwSignal<Vec<DmContact>> = RwSignal::new(Vec::new());
    let add_open = RwSignal::new(false);
    let navigate = use_navigate();

    // Load + refresh the contact list every ~3s so messages arriving via
    // the peer_receiver surface without a manual refresh.
    Effect::new(move |_| {
        spawn_local(async move {
            loop {
                contacts.set(list_contacts().await);
                wait_ms(3000).await;
            }
        });
    });

    view! {
        <div class="msgr-list">
            <header class="msgr-list-header">
                <h1 class="msgr-list-title">"Hey Chat"</h1>
                <button
                    type="button"
                    class="msgr-add-btn"
                    title="Add contact"
                    aria-label="Add contact"
                    on:click=move |_| add_open.set(true)
                >
                    <svg viewBox="0 0 24 24" class="msgr-icon" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M12 5v14M5 12h14" />
                    </svg>
                </button>
            </header>

            <div class="msgr-list-rows">
                {move || {
                    let list = contacts.get();
                    if list.is_empty() {
                        view! {
                            <div class="msgr-list-empty">
                                <p>"No conversations yet — add a contact."</p>
                            </div>
                        }.into_any()
                    } else {
                        view! {
                            <For
                                each=move || contacts.get()
                                key=|c| c.did.clone()
                                children={
                                    let navigate = navigate.clone();
                                    move |c: DmContact| {
                                        let navigate = navigate.clone();
                                        let did = c.did.clone();
                                        let is_active = active_did.get() == c.did;
                                        let row_class = if is_active {
                                            "msgr-row msgr-row-active"
                                        } else {
                                            "msgr-row"
                                        };
                                        let name = display_name(&c);
                                        let preview = if c.last_preview.is_empty() {
                                            "No messages yet".to_string()
                                        } else {
                                            c.last_preview.clone()
                                        };
                                        let unread = c.unread;
                                        view! {
                                            <button
                                                type="button"
                                                class=row_class
                                                on:click=move |_| {
                                                    navigate(
                                                        &format!("/chat/{}", did),
                                                        NavigateOptions::default(),
                                                    );
                                                }
                                            >
                                                <Avatar name=name.clone() />
                                                <div class="msgr-row-body">
                                                    <div class="msgr-row-top">
                                                        <span class="msgr-row-name">{name.clone()}</span>
                                                        <span class="msgr-row-time">{ts_short(c.last_ts)}</span>
                                                    </div>
                                                    <div class="msgr-row-bottom">
                                                        <span class="msgr-row-preview">{preview}</span>
                                                        {if unread > 0 {
                                                            view! {
                                                                <span class="msgr-badge">
                                                                    {if unread > 99 { "99+".to_string() } else { unread.to_string() }}
                                                                </span>
                                                            }.into_any()
                                                        } else {
                                                            ().into_any()
                                                        }}
                                                    </div>
                                                </div>
                                            </button>
                                        }
                                    }
                                }
                            />
                        }.into_any()
                    }
                }}
            </div>
        </div>
        <AddContactModal open=add_open />
    }
}

// ── Conversation ─────────────────────────────────────────────────────────
#[component]
fn Conversation(did: String) -> impl IntoView {
    let messages: RwSignal<Vec<DmMessage>> = RwSignal::new(Vec::new());
    let composer = RwSignal::new(String::new());
    let busy = RwSignal::new(false);

    // Load the conversation when the :did param changes + mark read on open.
    {
        let did_load = did.clone();
        Effect::new(move |_| {
            let d = did_load.clone();
            spawn_local(async move {
                let msgs = read_conversation(&d).await;
                messages.set(msgs);
                mark_read(&d).await;
            });
        });
    }

    // Poll the active conversation for incoming messages every ~3s.
    {
        let did_poll = did.clone();
        Effect::new(move |_| {
            let d = did_poll.clone();
            spawn_local(async move {
                loop {
                    wait_ms(3000).await;
                    let msgs = read_conversation(&d).await;
                    messages.set(msgs);
                }
            });
        });
    }

    let title = short_did(&did);
    let did_send = did.clone();
    let send = {
        let did = did_send.clone();
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
                // Optimistic: clear the input immediately, then refresh from
                // the engine (which appends the sent message).
                composer.set(String::new());
                let _ = send_message(&did, &text).await;
                let updated = read_conversation(&did).await;
                messages.set(updated);
                busy.set(false);
            });
        }
    };

    view! {
        <div class="msgr-conv">
            <header class="msgr-conv-header">
                <Avatar name=title.clone() />
                <div class="msgr-conv-title">
                    <span class="msgr-conv-name">{title.clone()}</span>
                    <span class="msgr-conv-status">"end-to-end encrypted"</span>
                </div>
            </header>

            <div class="msgr-conv-body">
                {move || {
                    let list = messages.get();
                    if list.is_empty() {
                        view! {
                            <div class="msgr-conv-empty">
                                <p>"No messages yet. Say hi 👋"</p>
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

            <Composer composer=composer busy=busy send=send.clone() />
        </div>
    }
}

#[component]
fn Bubble(m: DmMessage) -> impl IntoView {
    let row_class = if m.mine { "msgr-bubble-row msgr-bubble-row-mine" } else { "msgr-bubble-row" };
    let bubble_class = if m.mine { "msgr-bubble msgr-bubble-mine" } else { "msgr-bubble" };
    let ts_text = ts_short(m.ts);
    let lock = if m.encrypted { "🔒" } else { "!" };
    view! {
        <div class=row_class>
            <div class=bubble_class>
                <p class="msgr-bubble-text">{m.text}</p>
                <span class="msgr-bubble-meta">
                    {ts_text}" "<span class="msgr-bubble-lock">{lock}</span>
                </span>
            </div>
        </div>
    }
}

// ── Composer ─────────────────────────────────────────────────────────────
#[component]
fn Composer(
    composer: RwSignal<String>,
    busy: RwSignal<bool>,
    send: impl Fn() + 'static + Clone,
) -> impl IntoView {
    let on_input = move |ev: Event| {
        if let Some(t) = ev.target() {
            if let Ok(i) = t.dyn_into::<HtmlTextAreaElement>() {
                composer.set(i.value());
            }
        }
    };
    view! {
        <div class="msgr-composer">
            <textarea
                class="msgr-composer-input"
                rows="1"
                placeholder="Write a message…"
                prop:value=move || composer.get()
                on:input=on_input
                on:keydown={
                    let send = send.clone();
                    move |ev: KeyboardEvent| {
                        // Enter sends; Shift+Enter inserts a newline.
                        if ev.key() == "Enter" && !ev.shift_key() {
                            ev.prevent_default();
                            send();
                        }
                    }
                }
            ></textarea>
            <button
                type="button"
                class="msgr-send-btn"
                aria-label="Send"
                on:click={
                    let send = send.clone();
                    move |_| send()
                }
                prop:disabled=move || busy.get() || composer.get().trim().is_empty()
            >
                <svg viewBox="0 0 24 24" class="msgr-icon" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                    <path d="m22 2-7 20-4-9-9-4Z" />
                    <path d="M22 2 11 13" />
                </svg>
            </button>
        </div>
    }
}

// ── AddContactModal ──────────────────────────────────────────────────────
#[component]
fn AddContactModal(open: RwSignal<bool>) -> impl IntoView {
    // Tab: "create" | "accept".
    let tab = RwSignal::new("create".to_string());
    let invite_link = RwSignal::new(String::new());
    let paste = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let copied = RwSignal::new(false);
    let navigate = use_navigate();

    // Handlers are stashed in StoredValue so they're `Copy` and can be used
    // freely inside the (re-runnable) `<Show>` children + the reactive tab
    // blocks without move/FnOnce conflicts.
    let do_generate = StoredValue::new(move || {
        if busy.get() {
            return;
        }
        error.set(String::new());
        busy.set(true);
        spawn_local(async move {
            match generate_invite("").await {
                Ok(link) => invite_link.set(link),
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    });

    let do_accept = StoredValue::new({
        let navigate = navigate.clone();
        move || {
            if busy.get() {
                return;
            }
            let token = paste.get().trim().to_string();
            if token.is_empty() {
                return;
            }
            error.set(String::new());
            busy.set(true);
            let navigate = navigate.clone();
            spawn_local(async move {
                match accept_invite(&token).await {
                    Ok(did) => {
                        paste.set(String::new());
                        open.set(false);
                        navigate(&format!("/chat/{}", did), NavigateOptions::default());
                    }
                    Err(e) => error.set(e),
                }
                busy.set(false);
            });
        }
    });

    let copy_link = StoredValue::new(move || {
        let link = invite_link.get();
        if link.is_empty() {
            return;
        }
        if let Some(win) = web_sys::window() {
            let clipboard = win.navigator().clipboard();
            let _ = clipboard.write_text(&link);
            copied.set(true);
        }
    });

    // Escape-to-close. Re-arms whenever the modal transitions to open.
    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        let Some(win) = web_sys::window() else { return };
        let closure: wasm_bindgen::closure::Closure<dyn FnMut(KeyboardEvent)> =
            wasm_bindgen::closure::Closure::wrap(Box::new(move |ev: KeyboardEvent| {
                if ev.key() == "Escape" {
                    open.set(false);
                }
            }));
        let _ = win.add_event_listener_with_callback(
            "keydown",
            closure.as_ref().unchecked_ref(),
        );
        closure.forget();
    });

    // Reset transient state every time the modal opens.
    Effect::new(move |_| {
        if open.get() {
            error.set(String::new());
            copied.set(false);
        }
    });

    view! {
        <Show when=move || open.get() fallback=|| ().into_view()>
            <div
                class="msgr-modal-backdrop"
                on:click=move |_: MouseEvent| open.set(false)
            >
                <div
                    class="msgr-modal"
                    on:click=|ev: MouseEvent| ev.stop_propagation()
                >
                    <header class="msgr-modal-header">
                        <h3 class="msgr-modal-title">"Add contact"</h3>
                        <button
                            type="button"
                            class="msgr-modal-close"
                            aria-label="Close"
                            on:click=move |_| open.set(false)
                        >
                            <svg viewBox="0 0 24 24" class="msgr-icon" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                                <path d="M18 6 6 18M6 6l12 12" />
                            </svg>
                        </button>
                    </header>

                    <div class="msgr-tabs">
                        <button
                            type="button"
                            class=move || if tab.get() == "create" { "msgr-tab msgr-tab-active" } else { "msgr-tab" }
                            on:click=move |_| tab.set("create".into())
                        >
                            "Create invite"
                        </button>
                        <button
                            type="button"
                            class=move || if tab.get() == "accept" { "msgr-tab msgr-tab-active" } else { "msgr-tab" }
                            on:click=move |_| tab.set("accept".into())
                        >
                            "Accept invite"
                        </button>
                    </div>

                    {move || if tab.get() == "create" {
                        view! {
                            <div class="msgr-modal-body">
                                <p class="msgr-modal-hint">
                                    "Mint a one-time invite link and share it with someone. When they paste it back, you'll appear in each other's chats."
                                </p>
                                <button
                                    type="button"
                                    class="msgr-btn-primary"
                                    on:click=move |_| do_generate.with_value(|f| f())
                                    prop:disabled=move || busy.get()
                                >
                                    {move || if busy.get() { "Generating…" } else { "Generate invite link" }}
                                </button>
                                {move || {
                                    let link = invite_link.get();
                                    if link.is_empty() {
                                        ().into_any()
                                    } else {
                                        view! {
                                            <div class="msgr-invite-box">
                                                <textarea
                                                    class="msgr-invite-text"
                                                    readonly=true
                                                    prop:value=link.clone()
                                                ></textarea>
                                                <button
                                                    type="button"
                                                    class="msgr-btn-secondary"
                                                    on:click=move |_| copy_link.with_value(|f| f())
                                                >
                                                    {move || if copied.get() { "Copied!" } else { "Copy link" }}
                                                </button>
                                            </div>
                                        }.into_any()
                                    }
                                }}
                            </div>
                        }.into_any()
                    } else {
                        view! {
                            <div class="msgr-modal-body">
                                <p class="msgr-modal-hint">
                                    "Paste an invite link someone shared with you to start chatting."
                                </p>
                                <textarea
                                    class="msgr-invite-text"
                                    placeholder="hey-invite:…"
                                    prop:value=move || paste.get()
                                    on:input=move |ev: Event| {
                                        if let Some(t) = ev.target() {
                                            if let Ok(i) = t.dyn_into::<HtmlTextAreaElement>() {
                                                paste.set(i.value());
                                            }
                                        }
                                    }
                                    on:keydown=move |ev: KeyboardEvent| {
                                        if ev.key() == "Enter" && !ev.shift_key() {
                                            ev.prevent_default();
                                            do_accept.with_value(|f| f());
                                        }
                                    }
                                ></textarea>
                                <button
                                    type="button"
                                    class="msgr-btn-primary"
                                    on:click=move |_| do_accept.with_value(|f| f())
                                    prop:disabled=move || busy.get() || paste.get().trim().is_empty()
                                >
                                    {move || if busy.get() { "Accepting…" } else { "Accept invite" }}
                                </button>
                            </div>
                        }.into_any()
                    }}

                    {move || {
                        let m = error.get();
                        if m.is_empty() {
                            ().into_any()
                        } else {
                            view! { <p class="msgr-error msgr-modal-error">{m}</p> }.into_any()
                        }
                    }}
                </div>
            </div>
        </Show>
    }
}

// ── Avatar ───────────────────────────────────────────────────────────────
#[component]
fn Avatar(name: String) -> impl IntoView {
    let letters = initial_letters(&name);
    view! {
        <div class="msgr-avatar">{letters}</div>
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────
fn display_name(c: &DmContact) -> String {
    if !c.name.is_empty() {
        return c.name.clone();
    }
    if c.did.starts_with("pending:") {
        return "Awaiting reply…".into();
    }
    short_did(&c.did)
}

fn short_did(did: &str) -> String {
    if did.starts_with("pending:") {
        return "(invite pending)".into();
    }
    let s = did.strip_prefix("did:key:z").unwrap_or(did);
    if s.len() > 12 {
        format!("{}…", s.chars().take(12).collect::<String>())
    } else {
        s.into()
    }
}

fn initial_letters(name: &str) -> String {
    let s: String = name
        .chars()
        .filter(|c| c.is_alphanumeric())
        .take(2)
        .map(|c| c.to_uppercase().next().unwrap_or(c))
        .collect::<String>()
        .to_uppercase();
    if s.is_empty() {
        "?".into()
    } else {
        s
    }
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

async fn wait_ms(ms: i32) {
    let win = web_sys::window().unwrap();
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
    });
    let _ = JsFuture::from(promise).await;
}
