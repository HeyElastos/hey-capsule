// Inline SVG icons — Rust port of capsules/hey-social/client/src/components/icons.jsx.
// Hand-rolled subset; only the icons used by the ported pages/components.

use leptos::prelude::*;

#[component]
pub fn HomeIcon(#[prop(into, optional)] class: String) -> impl IntoView {
    let c = if class.is_empty() { "h-5 w-5".to_string() } else { class };
    view! {
        <svg viewBox="0 0 24 24" class=c fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M3 9.5 12 3l9 6.5V21a1 1 0 0 1-1 1h-5v-7H9v7H4a1 1 0 0 1-1-1z" />
        </svg>
    }
}

#[component]
pub fn ClipsIcon(#[prop(into, optional)] class: String) -> impl IntoView {
    let c = if class.is_empty() { "h-5 w-5".to_string() } else { class };
    view! {
        <svg viewBox="0 0 24 24" class=c fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <rect x="3" y="6" width="18" height="12" rx="2" />
            <path d="m10 9 5 3-5 3z" fill="currentColor" />
        </svg>
    }
}

#[component]
pub fn CameraIcon(#[prop(into, optional)] class: String) -> impl IntoView {
    let c = if class.is_empty() { "h-5 w-5".to_string() } else { class };
    view! {
        <svg viewBox="0 0 24 24" class=c fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M4 8h3l2-3h6l2 3h3a1 1 0 0 1 1 1v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V9a1 1 0 0 1 1-1z" />
            <circle cx="12" cy="13" r="3.5" />
        </svg>
    }
}

#[component]
pub fn ImageIcon(#[prop(into, optional)] class: String) -> impl IntoView {
    let c = if class.is_empty() { "h-5 w-5".to_string() } else { class };
    view! {
        <svg viewBox="0 0 24 24" class=c fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <rect x="3" y="3" width="18" height="18" rx="2" />
            <circle cx="9" cy="9" r="1.5" />
            <path d="M21 16 16 11l-7 7" />
        </svg>
    }
}

#[component]
pub fn UserIcon(#[prop(into, optional)] class: String) -> impl IntoView {
    let c = if class.is_empty() { "h-5 w-5".to_string() } else { class };
    view! {
        <svg viewBox="0 0 24 24" class=c fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="12" cy="8" r="4" />
            <path d="M4 21a8 8 0 0 1 16 0" />
        </svg>
    }
}

#[component]
pub fn ChatIcon(#[prop(into, optional)] class: String) -> impl IntoView {
    let c = if class.is_empty() { "h-5 w-5".to_string() } else { class };
    view! {
        <svg viewBox="0 0 24 24" class=c fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M21 12a8 8 0 0 1-12 6.9L4 21l1.7-4.6A8 8 0 1 1 21 12z" />
        </svg>
    }
}

#[component]
pub fn HeartIcon(#[prop(into, optional)] class: String, #[prop(optional)] filled: bool) -> impl IntoView {
    let c = if class.is_empty() { "h-5 w-5".to_string() } else { class };
    let fill = if filled { "currentColor" } else { "none" };
    view! {
        <svg viewBox="0 0 24 24" class=c fill=fill stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M20.8 7.3a5.4 5.4 0 0 0-9.5-3.6 5.4 5.4 0 0 0-9.5 3.6c0 6.3 9.5 11.2 9.5 11.2s9.5-4.9 9.5-11.2z" />
        </svg>
    }
}

#[component]
pub fn CommentIcon(#[prop(into, optional)] class: String) -> impl IntoView {
    let c = if class.is_empty() { "h-5 w-5".to_string() } else { class };
    view! {
        <svg viewBox="0 0 24 24" class=c fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M21 12a8 8 0 0 1-12 6.9L4 21l1.7-4.6A8 8 0 1 1 21 12z" />
        </svg>
    }
}

#[component]
pub fn PlusIcon(#[prop(into, optional)] class: String) -> impl IntoView {
    let c = if class.is_empty() { "h-5 w-5".to_string() } else { class };
    view! {
        <svg viewBox="0 0 24 24" class=c fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M12 5v14M5 12h14" />
        </svg>
    }
}

#[component]
pub fn LogoutIcon(#[prop(into, optional)] class: String) -> impl IntoView {
    let c = if class.is_empty() { "h-5 w-5".to_string() } else { class };
    view! {
        <svg viewBox="0 0 24 24" class=c fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4" />
            <path d="M16 17l5-5-5-5M21 12H9" />
        </svg>
    }
}

#[component]
pub fn BellIcon(#[prop(into, optional)] class: String) -> impl IntoView {
    let c = if class.is_empty() { "h-5 w-5".to_string() } else { class };
    view! {
        <svg viewBox="0 0 24 24" class=c fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9" />
            <path d="M10 21a2 2 0 0 0 4 0" />
        </svg>
    }
}

#[component]
pub fn SearchIcon(#[prop(into, optional)] class: String) -> impl IntoView {
    let c = if class.is_empty() { "h-5 w-5".to_string() } else { class };
    view! {
        <svg viewBox="0 0 24 24" class=c fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="11" cy="11" r="7" />
            <path d="m21 21-4.3-4.3" />
        </svg>
    }
}

#[component]
pub fn VideoIcon(#[prop(into, optional)] class: String) -> impl IntoView {
    let c = if class.is_empty() { "h-5 w-5".to_string() } else { class };
    view! {
        <svg viewBox="0 0 24 24" class=c fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="m23 7-7 5 7 5z" />
            <rect x="1" y="5" width="15" height="14" rx="2" />
        </svg>
    }
}

#[component]
pub fn ArrowRightIcon(#[prop(into, optional)] class: String) -> impl IntoView {
    let c = if class.is_empty() { "h-4 w-4".to_string() } else { class };
    view! {
        <svg viewBox="0 0 24 24" class=c fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M5 12h14M13 5l7 7-7 7" />
        </svg>
    }
}
