// SignIn — in the React reference, the sign-in UX IS the Landing page
// (one screen, passkey button + recovery fallback). Match that by
// delegating to Landing so /signin renders the exact same view as /.

use leptos::prelude::*;

use crate::pages::landing::Landing;

#[component]
pub fn SignIn() -> impl IntoView {
    view! { <Landing /> }
}
