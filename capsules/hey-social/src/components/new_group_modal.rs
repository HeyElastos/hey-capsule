// NewGroupModal — pick members from existing DM contacts + give the
// group a name. Creates the group locally and broadcasts a group.create.v1
// event so members see it pop into their list immediately.

use leptos::ev::KeyboardEvent;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;
use leptos_router::NavigateOptions;
use std::collections::HashSet;
use wasm_bindgen::JsCast;
use web_sys::HtmlInputElement;

use crate::api::dms::{list_contacts, DmContact};
use crate::api::groups::create_group;
use crate::components::Modal;

#[component]
pub fn NewGroupModal(open: RwSignal<bool>) -> impl IntoView {
    let name = RwSignal::new(String::new());
    let extra_did = RwSignal::new(String::new());
    let selected: RwSignal<HashSet<String>> = RwSignal::new(HashSet::new());
    let contacts: RwSignal<Vec<DmContact>> = RwSignal::new(Vec::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let navigate = use_navigate();

    // Load contacts when the modal opens.
    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        spawn_local(async move {
            contacts.set(list_contacts().await);
        });
    });

    let toggle = move |did: String| {
        selected.update(|s| {
            if s.contains(&did) {
                s.remove(&did);
            } else {
                s.insert(did);
            }
        });
    };

    let add_extra = move || {
        let d = extra_did.get().trim().to_string();
        if d.starts_with("did:key:z") {
            selected.update(|s| {
                s.insert(d);
            });
            extra_did.set(String::new());
        } else {
            error.set("DID must start with did:key:z…".into());
        }
    };

    let do_create = {
        let navigate = navigate.clone();
        move || {
            if busy.get() {
                return;
            }
            let n = name.get().trim().to_string();
            if n.is_empty() {
                error.set("Give the group a name.".into());
                return;
            }
            let members: Vec<String> = selected.get().iter().cloned().collect();
            if members.is_empty() {
                error.set("Add at least one member.".into());
                return;
            }
            error.set(String::new());
            busy.set(true);
            let navigate = navigate.clone();
            spawn_local(async move {
                match create_group(&n, members).await {
                    Ok(g) => {
                        busy.set(false);
                        open.set(false);
                        name.set(String::new());
                        selected.update(|s| s.clear());
                        navigate(&format!("/chat/g/{}", g.id), NavigateOptions::default());
                    }
                    Err(e) => {
                        error.set(e);
                        busy.set(false);
                    }
                }
            });
        }
    };

    view! {
        <Modal open=open>
            <div class="frosted-card frosted-card-strong p-5 space-y-3 max-h-[80vh] overflow-y-auto">
                <header class="flex items-center justify-between">
                    <h3 class="logo-handwritten text-4xl text-primary">"New group"</h3>
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

                <input
                    type="text"
                    class="frosted-input text-sm"
                    placeholder="Group name"
                    maxlength="60"
                    prop:value=move || name.get()
                    on:input=move |ev: web_sys::Event| {
                        if let Some(t) = ev.target() {
                            if let Ok(i) = t.dyn_into::<HtmlInputElement>() {
                                name.set(i.value());
                            }
                        }
                    }
                />

                <div>
                    <p class="text-[11px] uppercase tracking-wider text-muted mb-1.5">"Members"</p>
                    {move || {
                        let list = contacts.get();
                        if list.is_empty() {
                            view! {
                                <p class="text-xs text-muted py-3 text-center">"No DM contacts yet — paste a DID below to add one."</p>
                            }.into_any()
                        } else {
                            view! {
                                <ul class="space-y-1 max-h-40 overflow-y-auto">
                                    {list.into_iter().map(|c| {
                                        let did = c.did.clone();
                                        let did_for_toggle = did.clone();
                                        let label = if c.name.is_empty() {
                                            short_did(&c.did)
                                        } else {
                                            c.name.clone()
                                        };
                                        let did_eq = did.clone();
                                        let toggle = toggle.clone();
                                        view! {
                                            <li>
                                                <button
                                                    type="button"
                                                    on:click=move |_| toggle(did_for_toggle.clone())
                                                    class="w-full flex items-center gap-3 px-3 py-2 rounded-2xl hover:bg-white/10 transition-colors text-left"
                                                >
                                                    <span class="flex h-8 w-8 flex-none items-center justify-center rounded-full bg-gradient-to-br from-accent to-amber-600 text-accent-text text-xs font-bold">
                                                        {label.chars().take(2).collect::<String>().to_uppercase()}
                                                    </span>
                                                    <div class="flex-1 min-w-0">
                                                        <p class="text-sm text-primary truncate">{label}</p>
                                                        <p class="text-[10px] text-muted truncate font-mono">{short_did(&c.did)}</p>
                                                    </div>
                                                    <span
                                                        class="h-5 w-5 rounded-full border-2 flex items-center justify-center"
                                                        class:border-accent={ let did_eq = did_eq.clone(); move || selected.read().contains(&did_eq) }
                                                        class:bg-accent={ let did_eq = did_eq.clone(); move || selected.read().contains(&did_eq) }
                                                        class:border-surface={ let did_eq = did_eq.clone(); move || !selected.read().contains(&did_eq) }
                                                    >
                                                        {let did_eq = did_eq.clone(); move || if selected.read().contains(&did_eq) {
                                                            view! {
                                                                <svg viewBox="0 0 24 24" class="h-3 w-3 text-accent-text" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round">
                                                                    <path d="m5 12 5 5L20 7" />
                                                                </svg>
                                                            }.into_any()
                                                        } else { view! { <></> }.into_any() }}
                                                    </span>
                                                </button>
                                            </li>
                                        }
                                    }).collect::<Vec<_>>()}
                                </ul>
                            }.into_any()
                        }
                    }}
                </div>

                <div>
                    <p class="text-[11px] uppercase tracking-wider text-muted mb-1.5">"Or add by DID"</p>
                    <div class="flex gap-2">
                        <input
                            type="text"
                            class="frosted-input text-xs"
                            placeholder="did:key:z…"
                            prop:value=move || extra_did.get()
                            on:input=move |ev: web_sys::Event| {
                                if let Some(t) = ev.target() {
                                    if let Ok(i) = t.dyn_into::<HtmlInputElement>() {
                                        extra_did.set(i.value());
                                    }
                                }
                            }
                            on:keydown={
                                let add = add_extra.clone();
                                move |ev: KeyboardEvent| {
                                    if ev.key() == "Enter" {
                                        ev.prevent_default();
                                        add();
                                    }
                                }
                            }
                        />
                        <button
                            type="button"
                            on:click={
                                let add = add_extra.clone();
                                move |_| add()
                            }
                            class="unfrost rounded-full bg-white/10 border border-surface text-primary text-xs font-semibold px-3 py-1.5 shrink-0"
                        >"Add"</button>
                    </div>
                </div>

                // Selection summary chip count.
                {move || {
                    let n = selected.read().len();
                    if n == 0 { view! { <></> }.into_any() }
                    else {
                        view! {
                            <p class="text-[11px] text-muted">{format!("{n} member{}", if n == 1 { "" } else { "s" })}</p>
                        }.into_any()
                    }
                }}

                {move || {
                    let m = error.get();
                    if m.is_empty() { view! { <></> }.into_any() }
                    else { view! { <p class="text-xs text-red-400">{m}</p> }.into_any() }
                }}

                <button
                    type="button"
                    on:click={
                        let do_create = do_create.clone();
                        move |_| do_create()
                    }
                    prop:disabled=move || busy.get()
                    class="unfrost w-full rounded-full bg-accent hover:bg-amber-300 disabled:opacity-50 disabled:cursor-not-allowed text-accent-text font-semibold px-4 py-2.5 text-sm"
                >
                    {move || if busy.get() { "Creating…" } else { "Create group" }}
                </button>
            </div>
        </Modal>
    }
}

fn short_did(did: &str) -> String {
    let s = did.strip_prefix("did:key:z").unwrap_or(did);
    if s.len() > 16 { format!("{}…", s.chars().take(16).collect::<String>()) } else { s.into() }
}
