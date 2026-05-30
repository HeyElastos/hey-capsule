// SignIn — the sign-in UX IS the Landing page (runtime-only; identity comes
// from the Elastos runtime, there is no in-capsule auth). Delegate to Landing
// so /signin renders the exact same view as /.

use leptos::prelude::*;

use crate::pages::landing::Landing;

#[component]
pub fn SignIn() -> impl IntoView {
    view! { <Landing /> }
}
