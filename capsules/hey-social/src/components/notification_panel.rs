// NotificationPanel — centered popup of recent notifications, marks all
// as read on open. Uses the shared <Modal> shell for centering + Esc +
// fade-in animation.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::notifications::{self, Notification};
use crate::components::{Modal, NavLink};

#[component]
pub fn NotificationPanel(open: RwSignal<bool>) -> impl IntoView {
    let notes: RwSignal<Vec<Notification>> = RwSignal::new(Vec::new());

    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        spawn_local(async move {
            let list = notifications::list().await;
            notes.set(list);
            let _ = notifications::mark_all_read().await;
        });
    });

    view! {
        <Modal open=open>
            <div class="frosted-card frosted-card-strong p-5 max-h-[70vh] overflow-y-auto">
                <header class="flex items-baseline justify-between mb-3">
                    <h3 class="logo-handwritten text-4xl text-primary">"Notifications"</h3>
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
                {move || {
                    let list = notes.get();
                    if list.is_empty() {
                        view! {
                            <p class="text-sm text-muted text-center py-12">
                                "No notifications yet."
                            </p>
                        }.into_any()
                    } else {
                        view! {
                            <ul class="space-y-2">
                                {list.into_iter().map(|n| view! { <NotificationRow n=n on_delete=move |id: String| {
                                    spawn_local(async move {
                                        let _ = notifications::delete(&id).await;
                                        notes.update(|ns| ns.retain(|x| x.id != id));
                                    });
                                } /> }).collect::<Vec<_>>()}
                            </ul>
                        }.into_any()
                    }
                }}
            </div>
        </Modal>
    }
}

#[component]
fn NotificationRow(
    n: Notification,
    on_delete: impl Fn(String) + 'static + Send + Sync + Clone,
) -> impl IntoView {
    let from_label = if n.from_name.is_empty() {
        short_did(&n.from_did)
    } else {
        n.from_name.clone()
    };
    let label = match n.event_type.as_str() {
        "follow.request" => format!("{from_label} started following you"),
        "post.react" => format!(
            "{from_label} reacted {}",
            n.emoji.clone().unwrap_or_default()
        ),
        "dm.message" => format!("New message from {from_label}"),
        other => format!("{other} from {from_label}"),
    };
    let id = n.id.clone();
    let nav_href = match n.event_type.as_str() {
        "dm.message" => format!("/chat/{}", n.from_did),
        _ => format!("/profile/{}", n.from_did),
    };

    view! {
        <li class="rounded-2xl bg-white/10 border border-surface px-3 py-2.5 flex items-start gap-2 hover:bg-white/15 transition-colors">
            <NavLink href=nav_href class="flex-1 text-sm text-primary hover:underline">{label}</NavLink>
            <button
                type="button"
                on:click=move |_| on_delete(id.clone())
                class="icon-btn-ghost"
                aria-label="Dismiss"
                title="Dismiss"
            >
                <svg viewBox="0 0 24 24" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                    <path d="M18 6 6 18M6 6l12 12" />
                </svg>
            </button>
        </li>
    }
}

fn short_did(did: &str) -> String {
    let s = did.strip_prefix("did:key:z").unwrap_or(did);
    if s.len() > 16 {
        format!("{}…", s.chars().take(16).collect::<String>())
    } else {
        s.into()
    }
}
