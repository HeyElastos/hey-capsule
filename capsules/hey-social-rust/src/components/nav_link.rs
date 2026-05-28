// NavLink — a base-aware <a> wrapper.
//
// leptos_router 0.7's <A> component has a known quirk: when href starts with
// `/`, use_resolved_path returns the path as-is without prefixing the
// Router's base. The browser then either does a real navigation to
// `/<path>` (escaping our `/apps/hey-social-rust/` mount) OR <A>'s click
// interceptor calls navigate("/path") which does apply base — but only if
// the click is a left-click without modifiers and the href is reachable.
// In practice this has been flaky inside the runtime's iframe sandbox.
//
// NavLink sidesteps the issue by rendering a plain <a> + calling
// use_navigate from an onclick handler. The href attribute is set to the
// fully-resolved path so right-click → "Open in new tab" still works, and
// the click interceptor calls preventDefault + navigate() so SPA routing
// stays intact.

use leptos::prelude::*;
use leptos::ev::MouseEvent;
use leptos_router::hooks::use_navigate;
use leptos_router::NavigateOptions;

/// Compute the current Router base (e.g. "/apps/hey-social-rust") so href
/// attributes can be rendered with the full path. Mirrors lib.rs's
/// router_base() — kept inline to avoid a cyclic import.
fn current_base() -> String {
    let Some(win) = web_sys::window() else {
        return String::new();
    };
    let Ok(path) = win.location().pathname() else {
        return String::new();
    };
    let Some(idx) = path.find("/apps/") else {
        return String::new();
    };
    let after = &path[idx + 6..];
    let end = after.find('/').map(|j| idx + 6 + j).unwrap_or(path.len());
    path[..end].to_string()
}

#[component]
pub fn NavLink(
    /// Path to navigate to. Must start with `/` (absolute within the app).
    #[prop(into)]
    href: String,
    #[prop(into, optional)] class: String,
    #[prop(into, optional)] style: String,
    #[prop(into, optional)] title: String,
    #[prop(into, optional)] aria_label: String,
    children: Children,
) -> impl IntoView {
    let resolved = format!("{}{}", current_base(), href);
    let nav = use_navigate();
    let target = href.clone();
    let on_click = move |ev: MouseEvent| {
        // Let modified clicks / middle-clicks do their default thing
        // (open in new tab, etc.).
        if ev.default_prevented()
            || ev.button() != 0
            || ev.meta_key()
            || ev.ctrl_key()
            || ev.shift_key()
            || ev.alt_key()
        {
            return;
        }
        ev.prevent_default();
        nav(&target, NavigateOptions::default());
    };

    view! {
        <a
            href=resolved
            class=class
            style=style
            title=title
            aria-label=aria_label
            on:click=on_click
        >
            {children()}
        </a>
    }
}
