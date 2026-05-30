// LinkPhoneModal — shows a QR the Hey phone app scans to sign in.
//
// The QR encodes a `heyapp://connect?host=…&app=…&token=…` deep link built from
// this desktop's runtime origin + current Home launch token
// (hey_core::runtime::device_link_url). Scanning it lets the phone inherit this
// wallet-authorized session — no password, no seed on the phone. Uses the shared
// <Modal> shell (centering + Esc + backdrop close) and the same invite_qr_svg
// renderer as the DID / invite QRs.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::dms::invite_qr_svg;
use crate::components::Modal;
use crate::runtime::device_link_url;

#[component]
pub fn LinkPhoneModal(open: RwSignal<bool>) -> impl IntoView {
    // Auto-rotate the QR every 60s so the on-screen code stays fresh — the
    // device-link token self-expires (~120s), so a stale screenshot lapses.
    let tick = RwSignal::new(0u32);
    Effect::new(move |_| {
        spawn_local(async move {
            loop {
                crate::runtime::sleep_ms(60_000).await;
                tick.update(|t| *t += 1);
            }
        });
    });
    view! {
        <Modal open=open>
            {move || {
                // Recompute on each open + rotation so the QR always carries a
                // live, short-lived token.
                tick.get();
                let svg = device_link_url("hey-social").and_then(|link| invite_qr_svg(&link));
                view! {
                    <div class="frosted-card frosted-card-strong p-6 space-y-4 text-center">
                        <header class="flex items-center justify-between">
                            <h3 class="logo-handwritten text-4xl text-primary">"Link phone"</h3>
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

                        {match svg {
                            Some(svg) => view! {
                                <div
                                    class="mx-auto w-fit rounded-xl bg-white p-3 flex items-center justify-center"
                                    inner_html=svg
                                ></div>
                            }.into_any(),
                            None => view! {
                                <p class="text-sm text-muted">
                                    "Sign in first, then come back to link your phone."
                                </p>
                            }.into_any(),
                        }}

                        <p class="text-xs text-muted">
                            "Open Hey on your phone and scan this — no password; your phone borrows this device's wallet session. The code refreshes every minute and expires shortly after, so scan it now and don't share a screenshot."
                        </p>

                        <button
                            type="button"
                            on:click=move |_| open.set(false)
                            class="unfrost inline-flex items-center justify-center rounded-full bg-white/10 hover:bg-white/20 border border-surface text-primary px-6 py-1.5 text-xs font-semibold"
                        >
                            "Done"
                        </button>
                    </div>
                }
            }}
        </Modal>
    }
}
