// Posts — multi-photo upload with frosted preview cards.
//
// Mirrors capsules/hey-social/client/src/pages/Posts.jsx in spirit
// (multi-image carousel + caption + per-file progress) but keeps the
// Rust port leaner: no cassette/film-strip SVG decorations yet.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;
use leptos_router::NavigateOptions;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Event, HtmlInputElement, Url};

use crate::api::posts::{create_post, ipfs_upload_media, CreatePostArgs, MediaTile};
use crate::components::icons::{CameraIcon, ImageIcon};
use crate::components::{FloatingDock, TopHeader};

#[derive(Clone)]
struct StagedFile {
    id: String,
    bytes: Vec<u8>,
    name: String,
    mime: String,
    preview_url: String, // blob: URL, revoked on remove
}

// iPhone-style stack: top 3 cards visible, slightly rotated/offset so
// they look like a deck of photos. Bottom: horizontal thumbnail row to
// manage individual items (X to remove). "+N more" badge on the stack
// when there are >3 files.
#[component]
fn PreviewStack(
    staged: RwSignal<Vec<StagedFile>>,
    remove: impl Fn(String) + 'static + Clone + Send + Sync,
) -> impl IntoView {
    view! {
        <div class="flex flex-col items-center gap-4">
            // Stacked deck — purely visual, no interactions (manage via thumbnails below).
            <div class="relative w-44 h-44 sm:w-52 sm:h-52">
                {move || {
                    let files = staged.get();
                    let visible: Vec<_> = files.iter().rev().take(3).cloned().collect();
                    let total = files.len();
                    // 3 fixed transforms — top card straight on top.
                    let transforms = [
                        "rotate(-6deg) translate(-10px, 4px)",
                        "rotate(4deg) translate(8px, 2px)",
                        "rotate(0deg)",
                    ];
                    visible
                        .into_iter()
                        .enumerate()
                        .map(|(i, f)| {
                            // We render bottom-to-top; index 0 = bottom, top = last.
                            let depth = visible_depth(i, total.min(3));
                            let style = format!(
                                "transform: {}; z-index: {};",
                                transforms[depth],
                                depth + 1
                            );
                            let is_video = f.mime.starts_with("video/");
                            view! {
                                <div
                                    class="absolute inset-0 frosted-card overflow-hidden p-0 shadow-2xl shadow-slate-950/40 animate-fade-up"
                                    style=style
                                >
                                    {if is_video {
                                        view! {
                                            <video
                                                class="block w-full h-full object-cover bg-black"
                                                src=f.preview_url.clone()
                                                muted=true
                                            />
                                        }.into_any()
                                    } else {
                                        view! {
                                            <img
                                                class="block w-full h-full object-cover"
                                                src=f.preview_url.clone()
                                                alt=f.name.clone()
                                            />
                                        }.into_any()
                                    }}
                                </div>
                            }
                        })
                        .collect::<Vec<_>>()
                }}

                {move || {
                    let n = staged.get().len();
                    if n > 3 {
                        view! {
                            <span class="absolute -top-2 -right-2 z-20 inline-flex items-center justify-center rounded-full bg-accent text-accent-text text-xs font-bold px-2.5 py-1 shadow-lg">
                                {format!("+{}", n - 3)}
                            </span>
                        }.into_any()
                    } else { view! { <></> }.into_any() }
                }}
            </div>

            // Thumbnail row — small swipeable strip so users can drop
            // individual photos without losing the stack overview.
            <div class="w-full flex gap-2 overflow-x-auto pb-1 px-1 scroll-snap-x">
                <For
                    each=move || staged.get()
                    key=|f| f.id.clone()
                    children=move |f: StagedFile| {
                        let id_for_remove = f.id.clone();
                        let remove = remove.clone();
                        let click_remove = move |_| remove(id_for_remove.clone());
                        let is_video = f.mime.starts_with("video/");
                        view! {
                            <div class="relative shrink-0 w-16 h-16 rounded-xl overflow-hidden border border-surface bg-black/20">
                                {if is_video {
                                    view! { <video class="block w-full h-full object-cover" src=f.preview_url.clone() muted=true /> }.into_any()
                                } else {
                                    view! { <img class="block w-full h-full object-cover" src=f.preview_url.clone() alt=f.name.clone() /> }.into_any()
                                }}
                                <button
                                    type="button"
                                    on:click=click_remove
                                    class="absolute top-0.5 right-0.5 inline-flex h-5 w-5 items-center justify-center rounded-full bg-black/65 text-white hover:bg-black/85 transition-colors"
                                    aria-label="Remove"
                                    title="Remove"
                                >
                                    <svg viewBox="0 0 24 24" class="h-3 w-3" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                                        <path d="M18 6 6 18M6 6l12 12" />
                                    </svg>
                                </button>
                            </div>
                        }
                    }
                />
            </div>
        </div>
    }
}

// Map iteration index (top-most first) to the transform-table index
// so the top card always reads "straight" (rotation 0) and the lower
// cards fan out behind it.
fn visible_depth(i: usize, count: usize) -> usize {
    // i=0 is the topmost (most recently iterated). With count=3,
    // we want i=0 → transform 2 (straight), i=1 → transform 1, i=2 → 0.
    count - 1 - i
}

#[component]
pub fn Posts() -> impl IntoView {
    let caption = RwSignal::new(String::new());
    let staged: RwSignal<Vec<StagedFile>> = RwSignal::new(Vec::new());
    let busy = RwSignal::new(false);
    let progress = RwSignal::new(0u32);
    let error = RwSignal::new(String::new());
    let navigate = use_navigate();

    let on_file_change = move |ev: Event| {
        let Some(target) = ev.target() else { return };
        let Ok(input): Result<HtmlInputElement, _> = target.dyn_into() else {
            return;
        };
        let Some(files) = input.files() else { return };
        if files.length() == 0 {
            return;
        }
        error.set(String::new());
        for i in 0..files.length() {
            let Some(file) = files.get(i) else { continue };
            let name = file.name();
            let mime = file.type_();
            let preview = Url::create_object_url_with_blob(&file).unwrap_or_default();
            spawn_local(async move {
                let buf_promise = file.array_buffer();
                let Ok(buf_value) = JsFuture::from(buf_promise).await else {
                    return;
                };
                let array = js_sys::Uint8Array::new(&buf_value);
                let mut bytes = vec![0u8; array.length() as usize];
                array.copy_to(&mut bytes);
                staged.update(|v| {
                    v.push(StagedFile {
                        id: uuid::Uuid::new_v4().to_string(),
                        bytes,
                        name,
                        mime,
                        preview_url: preview,
                    });
                });
            });
        }
        // Reset the input so picking the same file again still fires change.
        input.set_value("");
    };

    let remove_staged = move |id: String| {
        staged.update(|v| {
            if let Some(idx) = v.iter().position(|s| s.id == id) {
                let removed = v.remove(idx);
                let _ = Url::revoke_object_url(&removed.preview_url);
            }
        });
    };

    let submit = move |_| {
        if busy.get() {
            return;
        }
        let files = staged.get();
        if files.is_empty() {
            error.set("Pick at least one photo or video first.".into());
            return;
        }
        let cap = caption.get();
        let navigate = navigate.clone();
        error.set(String::new());
        busy.set(true);
        progress.set(5);
        spawn_local(async move {
            let total = files.len() as u32;
            let mut tiles: Vec<MediaTile> = Vec::with_capacity(files.len());
            for (i, f) in files.iter().enumerate() {
                match ipfs_upload_media(&f.bytes, &f.name, &f.mime).await {
                    Ok(m) => tiles.push(m),
                    Err(e) => {
                        error.set(format!("IPFS upload failed: {e}"));
                        busy.set(false);
                        progress.set(0);
                        return;
                    }
                }
                let pct = 5 + ((i as u32 + 1) * 85 / total.max(1));
                progress.set(pct);
            }
            match create_post(CreatePostArgs {
                caption: cap,
                images: tiles,
            })
            .await
            {
                Ok(_) => {
                    progress.set(100);
                    busy.set(false);
                    // Revoke any preview URLs we created.
                    for f in &files {
                        let _ = Url::revoke_object_url(&f.preview_url);
                    }
                    staged.set(Vec::new());
                    navigate("/", NavigateOptions::default());
                }
                Err(e) => {
                    error.set(format!("Couldn't save post: {e}"));
                    busy.set(false);
                    progress.set(0);
                }
            }
        });
    };

    view! {
        <>
            <TopHeader />
            <FloatingDock />
            <div class="relative mx-auto max-w-3xl space-y-6 pl-24 pr-3 py-6 sm:pl-28 sm:pr-6 sm:py-10">
                <header class="px-1 animate-fade-in">
                    <h1 class="logo-handwritten text-4xl text-primary sm:text-5xl">
                        "Share a moment"
                    </h1>
                    <p class="mt-1 text-sm text-muted">
                        "Photo or short video. Stored on IPFS, pinned to your node, federated to your followers."
                    </p>
                </header>

                <div class="frosted-card p-6 space-y-4 animate-fade-up">
                    <div>
                        <span class="text-[11px] uppercase tracking-wider text-muted">
                            "Media"
                        </span>
                        <div class="mt-2 flex items-center gap-3 flex-wrap">
                            <label class="cursor-pointer inline-flex items-center gap-2 rounded-full bg-white/10 hover:bg-white/20 border border-surface px-4 py-2 text-sm font-medium text-primary">
                                <ImageIcon class="h-4 w-4" />
                                "Choose files"
                                <input
                                    type="file"
                                    class="sr-only"
                                    accept="image/*,video/*"
                                    multiple=true
                                    on:change=on_file_change
                                />
                            </label>
                            {move || {
                                let n = staged.read().len();
                                if n == 0 {
                                    view! { <span class="text-xs text-muted">"No files chosen"</span> }.into_any()
                                } else {
                                    view! { <span class="text-xs text-muted">{format!("{n} file{} ready", if n == 1 { "" } else { "s" })}</span> }.into_any()
                                }
                            }}
                        </div>
                    </div>

                    // Preview — iPhone-style stacked deck for the first 3
                    // photos (rotated slightly, fanned out), with a full
                    // thumbnail row below to manage individual items.
                    {move || {
                        let files = staged.get();
                        if files.is_empty() {
                            view! { <></> }.into_any()
                        } else {
                            view! { <PreviewStack staged=staged remove=remove_staged.clone() /> }.into_any()
                        }
                    }}

                    <div>
                        <span class="text-[11px] uppercase tracking-wider text-muted">
                            "Caption"
                        </span>
                        <textarea
                            class="frosted-input mt-2 text-sm"
                            rows="3"
                            maxlength="2200"
                            placeholder="Say something…"
                            on:input=move |ev: web_sys::Event| {
                                let target = ev.target().unwrap();
                                let ta = target
                                    .dyn_into::<web_sys::HtmlTextAreaElement>()
                                    .unwrap();
                                caption.set(ta.value());
                            }
                        />
                    </div>

                    {move || {
                        let p = progress.get();
                        if p == 0 { view! { <></> }.into_any() }
                        else {
                            view! {
                                <div class="h-1.5 w-full overflow-hidden rounded-full bg-white/10">
                                    <div class="h-full bg-accent transition-[width] duration-300" style=move || format!("width: {}%", progress.get())></div>
                                </div>
                            }.into_any()
                        }
                    }}

                    {move || {
                        let msg = error.get();
                        if msg.is_empty() { view! { <></> }.into_any() }
                        else {
                            view! { <p class="text-sm text-red-400">{msg}</p> }.into_any()
                        }
                    }}

                    <button
                        type="button"
                        on:click=submit
                        prop:disabled=move || busy.get()
                        class="unfrost w-full inline-flex items-center justify-center gap-2 rounded-full bg-accent px-6 py-3 text-sm font-semibold text-accent-text shadow-lg transition hover:bg-amber-300 disabled:cursor-not-allowed disabled:opacity-60"
                    >
                        <CameraIcon class="h-4 w-4" />
                        {move || if busy.get() { "Posting…" } else { "Post" }}
                    </button>
                </div>
            </div>
        </>
    }
}
