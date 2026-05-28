// Modal — shared shell for centered popups.
//
// All three app-level modals (NotificationPanel, SearchModal,
// AddFriendModal) use this so they get:
//   * Vertically + horizontally centered on every viewport
//   * Backdrop click closes
//   * Escape key closes (window keydown listener bound on open)
//   * Fade-in animation on mount
//
// Uses <Show> so the children closure is FnOnce-friendly per-open.

use leptos::ev::{KeyboardEvent, MouseEvent};
use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

#[component]
pub fn Modal(open: RwSignal<bool>, children: ChildrenFn) -> impl IntoView {
    // Escape-to-close. Re-arms on every transition to "open".
    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        let Some(win) = web_sys::window() else { return };
        let closure: Closure<dyn FnMut(KeyboardEvent)> =
            Closure::wrap(Box::new(move |ev: KeyboardEvent| {
                if ev.key() == "Escape" {
                    open.set(false);
                }
            }));
        let _ = win.add_event_listener_with_callback(
            "keydown",
            closure.as_ref().unchecked_ref(),
        );
        // Forget so the listener stays attached for this modal-open
        // lifetime; the open.get() guard above no-ops stale handlers.
        closure.forget();
    });

    view! {
        <Show when=move || open.get() fallback=|| view! { <></> }>
            <div
                class="modal-anchor fixed inset-0 z-50 flex items-start justify-center bg-black/40 backdrop-blur-sm px-4 pb-4 animate-fade-in"
                on:click=move |_: MouseEvent| open.set(false)
            >
                <div
                    class="animate-fade-up w-full max-w-md"
                    on:click=|ev: MouseEvent| ev.stop_propagation()
                >
                    {children()}
                </div>
            </div>
        </Show>
    }
}
