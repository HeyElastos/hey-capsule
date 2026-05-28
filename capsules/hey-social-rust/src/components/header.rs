// Sticky top header — Hey wordmark on left, photo/video tabs in center
// with .is-active glow, logout button on right.

use leptos::ev::MouseEvent;
use leptos::prelude::*;
use leptos_router::hooks::{use_location, use_navigate};
use leptos_router::NavigateOptions;

use crate::components::icons::{CameraIcon, LogoutIcon, VideoIcon};
use crate::components::NavLink;
use crate::session;

fn current_base() -> String {
    let Some(win) = web_sys::window() else { return String::new(); };
    let Ok(path) = win.location().pathname() else { return String::new(); };
    let Some(idx) = path.find("/apps/") else { return String::new(); };
    let after = &path[idx + 6..];
    let end = after.find('/').map(|j| idx + 6 + j).unwrap_or(path.len());
    path[..end].to_string()
}

#[component]
pub fn TopHeader() -> impl IntoView {
    let location = use_location();
    let navigate = use_navigate();
    let base = current_base();

    let is_videos = move || {
        let p = location.pathname.get();
        p.starts_with("/videos") || p == "/clips"
    };

    let logout = {
        let navigate = navigate.clone();
        move |_| {
            session::clear();
            navigate("/", NavigateOptions::default());
        }
    };

    // SPA-aware link click handler used for the photo/video tabs which
    // need reactive class strings (so we can't use the simpler NavLink
    // wrapper with its static-class signature).
    let click_to = {
        let navigate = navigate.clone();
        move |path: &'static str| {
            let navigate = navigate.clone();
            move |ev: MouseEvent| {
                if ev.default_prevented()
                    || ev.button() != 0
                    || ev.meta_key()
                    || ev.ctrl_key()
                    || ev.shift_key()
                    || ev.alt_key()
                { return; }
                ev.prevent_default();
                navigate(path, NavigateOptions::default());
            }
        }
    };

    view! {
        <header class="sticky top-0 z-30 bg-surface-soft/95 backdrop-blur-xl shadow-[0_16px_40px_-18px_rgba(0,0,0,0.15)]">
            <div class="mx-auto flex max-w-6xl items-center justify-between px-4 py-3 sm:px-6">
                <NavLink
                    href="/"
                    class="text-3xl font-semibold text-primary logo-handwritten sm:text-5xl"
                >
                    "Hey"
                </NavLink>

                <nav class="flex flex-1 items-center justify-center gap-8 text-sm sm:gap-12">
                    <a
                        href=format!("{}/", base)
                        class="icon-btn tab-icon"
                        class:is-active=move || !is_videos()
                        aria-label="Photos"
                        on:click=click_to.clone()("/")
                    >
                        <CameraIcon class="h-6 w-6" />
                    </a>
                    <a
                        href=format!("{}/videos", base)
                        class="icon-btn tab-icon"
                        class:is-active=is_videos
                        aria-label="Videos"
                        on:click=click_to.clone()("/videos")
                    >
                        <VideoIcon class="h-6 w-6" />
                    </a>
                </nav>

                <div class="flex items-center gap-2">
                    <button
                        type="button"
                        on:click=logout
                        class="icon-btn"
                        aria-label="Log out"
                    >
                        <LogoutIcon class="h-5 w-5" />
                    </button>
                </div>
            </div>
        </header>
    }
}
