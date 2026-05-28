// AddFriendModal — paste a DID, follow that peer. Uses the shared
// <Modal> shell for centering + Esc + fade-in.

use leptos::ev::KeyboardEvent;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;
use web_sys::HtmlInputElement;

use crate::api::profile::follow_user;
use crate::components::Modal;

#[component]
pub fn AddFriendModal(open: RwSignal<bool>) -> impl IntoView {
    let did_input = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let ok_msg = RwSignal::new(String::new());

    let on_input = move |ev: web_sys::Event| {
        if let Some(t) = ev.target() {
            if let Ok(i) = t.dyn_into::<HtmlInputElement>() {
                did_input.set(i.value());
            }
        }
    };

    let do_follow = move || {
        if busy.get() {
            return;
        }
        let d = did_input.get().trim().to_string();
        if !d.starts_with("did:key:z") {
            error.set("Enter a did:key:z… identity.".into());
            return;
        }
        error.set(String::new());
        ok_msg.set(String::new());
        busy.set(true);
        spawn_local(async move {
            match follow_user(&d).await {
                Ok(()) => {
                    ok_msg.set("Following.".into());
                    did_input.set(String::new());
                }
                Err(e) => error.set(format!("{e}")),
            }
            busy.set(false);
        });
    };

    view! {
        <Modal open=open>
            <div class="frosted-card frosted-card-strong p-5 space-y-3">
                <header class="flex items-center justify-between">
                    <h3 class="logo-handwritten text-4xl text-primary">"Add a friend"</h3>
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
                <p class="text-xs text-muted">"Paste their did:key from a share card or QR code."</p>
                <input
                    type="text"
                    class="frosted-input text-sm"
                    placeholder="did:key:z…"
                    prop:value=move || did_input.get()
                    on:input=on_input
                    on:keydown={
                        let do_follow = do_follow.clone();
                        move |ev: KeyboardEvent| {
                            if ev.key() == "Enter" {
                                ev.prevent_default();
                                do_follow();
                            }
                        }
                    }
                />
                {move || {
                    let m = error.get();
                    if m.is_empty() { view! { <></> }.into_any() }
                    else { view! { <p class="text-xs text-red-400">{m}</p> }.into_any() }
                }}
                {move || {
                    let m = ok_msg.get();
                    if m.is_empty() { view! { <></> }.into_any() }
                    else { view! { <p class="text-xs text-emerald-400">{m}</p> }.into_any() }
                }}
                <button
                    type="button"
                    on:click={
                        let do_follow = do_follow.clone();
                        move |_| do_follow()
                    }
                    prop:disabled=move || busy.get()
                    class="unfrost w-full rounded-full bg-accent hover:bg-amber-300 disabled:opacity-50 disabled:cursor-not-allowed text-accent-text font-semibold px-4 py-2.5 text-sm transition-colors"
                >
                    {move || if busy.get() { "Following…" } else { "Follow" }}
                </button>
            </div>
        </Modal>
    }
}
