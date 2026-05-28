// SearchModal — paste-or-type-a-DID to navigate to a profile. Uses the
// shared <Modal> shell for centering + Esc + fade-in.

use leptos::prelude::*;
use leptos::ev::KeyboardEvent;
use leptos_router::hooks::use_navigate;
use leptos_router::NavigateOptions;
use wasm_bindgen::JsCast;
use web_sys::HtmlInputElement;

use crate::components::icons::SearchIcon;
use crate::components::Modal;

#[component]
pub fn SearchModal(open: RwSignal<bool>) -> impl IntoView {
    let query = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let navigate = use_navigate();

    let on_input = move |ev: web_sys::Event| {
        if let Some(t) = ev.target() {
            if let Ok(i) = t.dyn_into::<HtmlInputElement>() {
                query.set(i.value());
            }
        }
    };

    let go = move || {
        let q = query.get().trim().to_string();
        if !q.starts_with("did:key:z") {
            error.set("Enter a did:key:z… identity.".into());
            return;
        }
        error.set(String::new());
        open.set(false);
        query.set(String::new());
        navigate.clone()(&format!("/profile/{q}"), NavigateOptions::default());
    };

    view! {
        <Modal open=open>
            <div class="frosted-card frosted-card-strong p-5 space-y-3">
                <header class="flex items-center justify-between">
                    <h3 class="logo-handwritten text-4xl text-primary">"Find someone"</h3>
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
                <p class="text-xs text-muted">"Paste a did:key from a share card or QR code."</p>
                <div class="flex items-center gap-2">
                    <SearchIcon class="h-5 w-5 text-muted shrink-0" />
                    <input
                        type="text"
                        class="frosted-input text-sm"
                        placeholder="did:key:z…"
                        prop:value=move || query.get()
                        on:input=on_input
                        on:keydown={
                            let go = go.clone();
                            move |ev: KeyboardEvent| {
                                if ev.key() == "Enter" {
                                    ev.prevent_default();
                                    go();
                                }
                            }
                        }
                    />
                </div>
                {move || {
                    let m = error.get();
                    if m.is_empty() { view! { <></> }.into_any() }
                    else { view! { <p class="text-xs text-red-400">{m}</p> }.into_any() }
                }}
                <button
                    type="button"
                    on:click={
                        let go = go.clone();
                        move |_| go()
                    }
                    class="unfrost w-full rounded-full bg-accent hover:bg-amber-300 text-accent-text font-semibold px-4 py-2.5 text-sm transition-colors"
                >
                    "Open profile"
                </button>
            </div>
        </Modal>
    }
}
