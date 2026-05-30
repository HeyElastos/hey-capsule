// Posts — upload + caption screen with a big drop-zone, polaroid-style
// preview grid, drag-and-drop, stats bar, and a shimmering submit CTA.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;
use leptos_router::NavigateOptions;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{DragEvent, Event, FileList, HtmlInputElement, Url};

use crate::api::posts::{create_post, ipfs_upload_media, CreatePostArgs, MediaTile};
use crate::components::icons::{CameraIcon, ImageIcon};

#[derive(Clone)]
struct StagedFile {
    id: String,
    bytes: Vec<u8>,
    name: String,
    mime: String,
    preview_url: String, // blob: URL, revoked on remove
}

#[component]
pub fn Posts() -> impl IntoView {
    let caption = RwSignal::new(String::new());
    let staged: RwSignal<Vec<StagedFile>> = RwSignal::new(Vec::new());
    let busy = RwSignal::new(false);
    let progress = RwSignal::new(0u32);
    let error = RwSignal::new(String::new());
    let dragging = RwSignal::new(false);
    let navigate = use_navigate();

    let ingest_files = move |files: FileList| {
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
                let Ok(buf_value) = JsFuture::from(file.array_buffer()).await else {
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
    };

    let on_file_change = {
        let ingest = ingest_files.clone();
        move |ev: Event| {
            let Some(target) = ev.target() else { return };
            let Ok(input): Result<HtmlInputElement, _> = target.dyn_into() else { return };
            if let Some(files) = input.files() {
                ingest(files);
            }
            input.set_value("");
        }
    };

    let on_drop = {
        let ingest = ingest_files.clone();
        move |ev: DragEvent| {
            ev.prevent_default();
            dragging.set(false);
            if let Some(dt) = ev.data_transfer() {
                if let Some(files) = dt.files() {
                    ingest(files);
                }
            }
        }
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

    let total_bytes = Memo::new(move |_| staged.read().iter().map(|f| f.bytes.len()).sum::<usize>());
    let file_count = Memo::new(move |_| staged.read().len());

    view! {
        <>
            <div class="page-enter relative mx-auto max-w-3xl pl-24 pr-3 py-6 sm:pl-28 sm:pr-6 sm:py-10 overflow-hidden">

                // ── Hero ──────────────────────────────────────────
                <UploadHero />

                // ── Drop zone (when empty) OR polaroid grid (when staged) ──
                <div class="mt-6 relative z-10">
                    {move || {
                        let count = file_count.get();
                        if count == 0 {
                            view! {
                                <DropZone
                                    dragging=dragging
                                    on_drop=on_drop.clone()
                                    on_file_change=on_file_change.clone()
                                />
                            }.into_any()
                        } else {
                            view! {
                                <PolaroidGrid
                                    staged=staged
                                    remove=remove_staged.clone()
                                    on_file_change=on_file_change.clone()
                                    dragging=dragging
                                    on_drop=on_drop.clone()
                                />
                            }.into_any()
                        }
                    }}
                </div>

                // ── Stats bar (only when files staged) ─────────────
                {move || {
                    let n = file_count.get();
                    if n == 0 { view! { <></> }.into_any() }
                    else {
                        let bytes = total_bytes.get();
                        view! {
                            <div class="mt-4 flex flex-wrap items-center justify-center gap-3 text-xs text-muted animate-fade-in">
                                <span class="inline-flex items-center gap-1.5">
                                    <ImageIcon class="h-4 w-4 text-accent" />
                                    <strong class="text-primary">{n}</strong>
                                    {if n == 1 { " file" } else { " files" }}
                                </span>
                                <span class="text-muted/50">"·"</span>
                                <span><strong class="text-primary">{format_size(bytes)}</strong>" original"</span>
                                <span class="text-muted/50">"·"</span>
                                <span title="Photos are normalized to AVIF q80 before pinning to IPFS. Typical savings: 25-40% vs WebP at similar quality.">
                                    "→ "<strong class="text-emerald-400">"AVIF compression"</strong>
                                </span>
                            </div>
                        }.into_any()
                    }
                }}

                // ── Caption ────────────────────────────────────────
                <div class="mt-6 frosted-card p-5 sm:p-6 animate-fade-up">
                    <span class="text-[11px] uppercase tracking-wider text-muted">"Caption"</span>
                    <textarea
                        class="frosted-input mt-2 text-base"
                        rows="3"
                        maxlength="2200"
                        placeholder="Say something about this moment…"
                        on:input=move |ev: Event| {
                            let target = ev.target().unwrap();
                            let ta = target.dyn_into::<web_sys::HtmlTextAreaElement>().unwrap();
                            caption.set(ta.value());
                        }
                    />
                    <div class="mt-2 flex justify-end text-[10px] text-muted">
                        {move || format!("{} / 2200", caption.read().chars().count())}
                    </div>
                </div>

                // ── Progress + error ──────────────────────────────
                {move || {
                    let p = progress.get();
                    if p == 0 { view! { <></> }.into_any() }
                    else {
                        view! {
                            <div class="mt-4 space-y-1">
                                <div class="h-2 w-full overflow-hidden rounded-full bg-white/10 border border-surface">
                                    <div
                                        class="h-full bg-gradient-to-r from-amber-400 via-rose-400 to-violet-400 transition-[width] duration-300"
                                        style=move || format!(
                                            "width: {}%; background-size: 200% 100%; animation: progress-shimmer 2s ease-in-out infinite",
                                            progress.get()
                                        )
                                    />
                                </div>
                                <p class="text-[10px] text-muted text-center">
                                    {move || if progress.get() < 100 {
                                        format!("Pinning to IPFS… {}%", progress.get())
                                    } else { "Done!".to_string() }}
                                </p>
                            </div>
                        }.into_any()
                    }
                }}

                {move || {
                    let msg = error.get();
                    if msg.is_empty() { view! { <></> }.into_any() }
                    else {
                        view! {
                            <p class="mt-3 text-sm text-red-400 text-center animate-fade-in">{msg}</p>
                        }.into_any()
                    }
                }}

                // ── Submit ────────────────────────────────────────
                <button
                    type="button"
                    on:click=submit
                    prop:disabled=move || busy.get()
                    class="shimmer-cta unfrost mt-6 w-full inline-flex items-center justify-center gap-2 rounded-full bg-gradient-to-r from-amber-400 via-rose-400 to-violet-500 px-6 py-4 text-base font-bold text-white shadow-2xl shadow-amber-500/20 transition hover:shadow-amber-500/40 disabled:cursor-not-allowed disabled:opacity-60"
                >
                    {move || if busy.get() {
                        view! {
                            <svg viewBox="0 0 24 24" class="spinner h-5 w-5" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" aria-hidden="true">
                                <path d="M21 12a9 9 0 1 1-6.2-8.5" />
                            </svg>
                        }.into_any()
                    } else {
                        view! { <CameraIcon class="h-5 w-5" /> }.into_any()
                    }}
                    {move || if busy.get() { "Posting…" } else { "Share the moment" }}
                </button>

                <p class="mt-3 text-center text-[10px] text-muted">
                    "Pinned to IPFS · federated via Carrier · "
                    <span class="text-emerald-400">"end-to-end signed"</span>
                </p>
            </div>
        </>
    }
}

#[component]
fn UploadHero() -> impl IntoView {
    view! {
        <header class="relative px-1 animate-fade-in flex items-start gap-4 sm:gap-6">
            <div class="flex-1 min-w-0">
                <h1 class="logo-handwritten text-6xl sm:text-7xl text-primary">
                    "Share a moment"
                </h1>
                <p class="mt-2 text-sm text-muted max-w-md">
                    "Drop a photo or short clip. We'll pin it to IPFS, sign it with your DID, and federate it to your followers."
                </p>
            </div>

            // Decorative mini-polaroids hovering next to the title.
            // Visible inside the hero (no negative top offsets), each
            // tilted differently and drifting with .float-soft so they
            // feel pinned to a corkboard.
            <div class="hidden sm:flex shrink-0 items-end gap-2 pr-1 pt-2" aria-hidden="true">
                <MiniPolaroid
                    rotation="-8deg"
                    delay="0s"
                    gradient="bg-gradient-to-br from-amber-300 via-rose-400 to-fuchsia-500"
                >
                    // Sun over horizon
                    <svg viewBox="0 0 32 32" class="h-full w-full" fill="none">
                        <circle cx="16" cy="13" r="5" fill="rgba(255,255,255,0.85)" />
                        <path d="M0 24h32" stroke="rgba(255,255,255,0.7)" stroke-width="2" />
                        <path d="M-2 28h36" stroke="rgba(255,255,255,0.45)" stroke-width="2" />
                    </svg>
                </MiniPolaroid>
                <MiniPolaroid
                    rotation="4deg"
                    delay="-1.2s"
                    gradient="bg-gradient-to-br from-sky-400 via-cyan-400 to-emerald-400"
                >
                    // Mountain
                    <svg viewBox="0 0 32 32" class="h-full w-full" fill="none">
                        <path d="M0 28L8 16l6 8 6-12 12 16z" fill="rgba(255,255,255,0.85)" />
                        <circle cx="22" cy="8" r="2.5" fill="rgba(255,255,255,0.9)" />
                    </svg>
                </MiniPolaroid>
                <MiniPolaroid
                    rotation="-3deg"
                    delay="-2.4s"
                    gradient="bg-gradient-to-br from-violet-400 via-fuchsia-400 to-rose-400"
                >
                    // Heart on a film frame
                    <svg viewBox="0 0 32 32" class="h-full w-full" fill="rgba(255,255,255,0.92)">
                        <path d="M16 28S5 21 5 13a5 5 0 0 1 11-2 5 5 0 0 1 11 2c0 8-11 15-11 15z" />
                    </svg>
                </MiniPolaroid>
            </div>
        </header>
    }
}

#[component]
fn MiniPolaroid(
    #[prop(into)] rotation: String,
    #[prop(into)] delay: String,
    #[prop(into)] gradient: String,
    children: Children,
) -> impl IntoView {
    let outer_style = format!("transform: rotate({rotation}); animation-delay: {delay};");
    let inner_class =
        format!("relative w-12 h-12 sm:w-14 sm:h-14 rounded-sm overflow-hidden {gradient}");
    view! {
        <div
            class="float-soft polaroid !rotate-0 !p-1.5 !pb-4 !rounded shadow-lg shadow-slate-950/40"
            style=outer_style
        >
            <div class=inner_class>
                {children()}
            </div>
        </div>
    }
}

#[component]
fn DropZone(
    dragging: RwSignal<bool>,
    on_drop: impl Fn(DragEvent) + 'static + Clone + Send + Sync,
    on_file_change: impl Fn(Event) + 'static + Clone + Send + Sync,
) -> impl IntoView {
    view! {
        <label
            class="drop-zone block cursor-pointer rounded-3xl border-2 border-dashed border-surface bg-white/5 hover:bg-white/10 px-6 py-12 sm:py-16 text-center animate-fade-up"
            class:drop-zone--dragging=move || dragging.get()
            on:dragenter=move |ev: DragEvent| { ev.prevent_default(); dragging.set(true); }
            on:dragover=move |ev: DragEvent| { ev.prevent_default(); dragging.set(true); }
            on:dragleave=move |ev: DragEvent| { ev.prevent_default(); dragging.set(false); }
            on:drop=on_drop.clone()
        >
            <div class="pulse-glow mx-auto inline-flex h-20 w-20 items-center justify-center rounded-3xl bg-gradient-to-br from-amber-400/30 via-rose-400/30 to-violet-500/30 border border-white/20 text-accent">
                <CameraIcon class="h-10 w-10" />
            </div>
            <h2 class="mt-5 text-xl font-semibold text-primary">
                {move || if dragging.get() { "Drop to add" } else { "Drop photos here" }}
            </h2>
            <p class="mt-1 text-sm text-muted">
                "or "
                <span class="text-accent font-semibold underline-offset-2 hover:underline">
                    "click to browse"
                </span>
            </p>
            <p class="mt-4 text-[11px] text-muted/70">
                "Images + video · multi-select · stored sovereign on your own node"
            </p>
            <input
                type="file"
                class="sr-only"
                accept="image/*,video/*"
                multiple=true
                on:change=on_file_change.clone()
            />
        </label>
    }
}

/// Cumulative fan offset (in px) for the card at `index` in the staged pile.
///
/// The first ~5 cards spread by a generous step (42px right, 18px down) so each
/// polaroid clearly peeks out from under the next; past that the step tightens
/// (to 14px / 6px) so a big pile stays tidy and doesn't run off-screen. Returns
/// the running totals `(dx, dy)`.

#[component]
fn PolaroidGrid(
    staged: RwSignal<Vec<StagedFile>>,
    remove: impl Fn(String) + 'static + Clone + Send + Sync,
    on_file_change: impl Fn(Event) + 'static + Clone + Send + Sync,
    dragging: RwSignal<bool>,
    on_drop: impl Fn(DragEvent) + 'static + Clone + Send + Sync,
) -> impl IntoView {
    // Computed OUTSIDE the view! macro: the `Vec<_>` turbofish / type
    // angle-brackets confuse the macro parser into reading them as tags.
    let pile_items = move || {
        let v: Vec<(usize, StagedFile)> = staged.get().into_iter().enumerate().collect();
        v
    };
    view! {
        <div
            class="drop-zone rounded-3xl p-4 sm:p-5 bg-white/5 border-2 border-dashed border-surface"
            class:drop-zone--dragging=move || dragging.get()
            on:dragenter=move |ev: DragEvent| { ev.prevent_default(); dragging.set(true); }
            on:dragover=move |ev: DragEvent| { ev.prevent_default(); dragging.set(true); }
            on:dragleave=move |ev: DragEvent| { ev.prevent_default(); dragging.set(false); }
            on:drop=on_drop.clone()
        >
            // Overlapping fan of classic white instant-photos: each card after
            // the first slides LEFT over the previous and tilts a touch, so the
            // photos lay across each other and stay all-visible (newest on top).
            // Hovering a card lifts it clear so the ones beneath are reachable.
            <div class="staged-fan">
                <For
                    each=pile_items
                    key=|(_, f)| f.id.clone()
                    children=move |(i, f): (usize, StagedFile)| {
                        let id_for_remove = f.id.clone();
                        let remove = remove.clone();
                        let click_remove = move |_| remove(id_for_remove.clone());
                        let is_video = f.mime.starts_with("video/");
                        let tilts = [-5.0_f64, 3.0, -2.0, 5.0, -3.0, 2.0];
                        let rot = tilts[i % tilts.len()];
                        let ml = if i == 0 { 0.0 } else { -2.6 };
                        let z = i + 1;
                        let rest = format!(
                            "margin-left:{ml}rem; z-index:{z}; transform: rotate({rot}deg); \
                             transition: transform .2s cubic-bezier(.22,1,.36,1);",
                        );
                        let hover = format!(
                            "margin-left:{ml}rem; z-index:999; \
                             transform: rotate(0deg) translateY(-12px) scale(1.06); \
                             transition: transform .2s cubic-bezier(.22,1,.36,1);",
                        );
                        let hovered = RwSignal::new(false);
                        view! {
                            <div
                                class="animate-fade-in"
                                style=move || if hovered.get() { hover.clone() } else { rest.clone() }
                                on:mouseenter=move |_| hovered.set(true)
                                on:mouseleave=move |_| hovered.set(false)
                            >
                                <div class="staged-polaroid">
                                    <div class="staged-photo">
                                        {if is_video {
                                            view! { <video src=f.preview_url.clone() muted=true /> }.into_any()
                                        } else {
                                            view! { <img src=f.preview_url.clone() alt=f.name.clone() /> }.into_any()
                                        }}
                                        <button
                                            type="button"
                                            on:click=click_remove
                                            class="absolute top-1 right-1 z-10 inline-flex h-6 w-6 items-center justify-center rounded-full bg-black/60 text-white hover:bg-rose-500 backdrop-blur-sm transition-colors"
                                            aria-label="Remove"
                                            title="Remove"
                                        >
                                            <svg viewBox="0 0 24 24" class="h-3 w-3" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18M6 6l12 12" /></svg>
                                        </button>
                                        {if is_video {
                                            view! { <span class="pointer-events-none absolute bottom-1 left-1 z-10 inline-flex items-center gap-1 rounded-full bg-black/70 px-1.5 py-0.5 text-[9px] text-white"><svg viewBox="0 0 24 24" class="h-2.5 w-2.5" fill="currentColor"><path d="M5 4l14 8-14 8z" /></svg>"Video"</span> }.into_any()
                                        } else { view! { <></> }.into_any() }}
                                    </div>
                                </div>
                            </div>
                        }
                    }
                />

                // "Add more" tile at the end of the fan.
                <label class="staged-addmore cursor-pointer flex flex-col items-center justify-center rounded-lg border-2 border-dashed border-surface bg-white/5 hover:bg-white/10 transition-colors text-muted hover:text-accent">
                    <svg viewBox="0 0 24 24" class="h-7 w-7" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M12 5v14M5 12h14" />
                    </svg>
                    <span class="mt-1 text-xs font-semibold">"Add more"</span>
                    <input
                        type="file"
                        class="sr-only"
                        accept="image/*,video/*"
                        multiple=true
                        on:change=on_file_change.clone()
                    />
                </label>
            </div>

            <p class="mt-4 text-center text-[10px] text-muted/70">
                "Drag more photos in, or click "<strong class="text-muted">"Add more"</strong>"."
            </p>
        </div>
    }
}

fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
